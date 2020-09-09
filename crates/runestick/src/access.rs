use std::cell::Cell;
use std::fmt;
use std::future::Future;
use std::marker;
use std::ops;
use std::pin::Pin;
use std::task::{Context, Poll};
use thiserror::Error;

/// Flag to used to mark access as taken.
const TAKEN: isize = isize::max_value();

/// An error raised while downcasting.
#[derive(Debug, Error)]
pub enum AccessError {
    /// Error raised when we expect a specific external type but got another.
    #[error("expected data of type `{expected}`, but found `{actual}`")]
    UnexpectedType {
        /// The type that was expected.
        expected: &'static str,
        /// The type that was found.
        actual: &'static str,
    },
    /// Trying to access an inaccessible reference.
    #[error("{error}")]
    NotAccessibleRef {
        /// Source error.
        #[from]
        error: NotAccessibleRef,
    },
    /// Trying to access an inaccessible mutable reference.
    #[error("{error}")]
    NotAccessibleMut {
        /// Source error.
        #[from]
        error: NotAccessibleMut,
    },
    /// Trying to access an inaccessible taking.
    #[error("{error}")]
    NotAccessibleTake {
        /// Source error.
        #[from]
        error: NotAccessibleTake,
    },
}

/// Error raised when tried to access for shared access but it was not
/// accessible.
#[derive(Debug, Error)]
#[error("cannot read, value is {0}")]
pub struct NotAccessibleRef(Snapshot);

/// Error raised when tried to access for exclusive access but it was not
/// accessible.
#[derive(Debug, Error)]
#[error("cannot write, value is {0}")]
pub struct NotAccessibleMut(Snapshot);

/// Error raised when tried to access the guarded data for taking.
///
/// This requires exclusive access, but it's a scenario we structure separately
/// for diagnostics purposes.
#[derive(Debug, Error)]
#[error("cannot take, value is {0}")]
pub struct NotAccessibleTake(Snapshot);

/// Snapshot that can be used to indicate how the value was being accessed at
/// the time of an error.
#[derive(Debug)]
#[repr(transparent)]
pub struct Snapshot(isize);

impl fmt::Display for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            0 => write!(f, "fully accessible"),
            1 => write!(f, "exclusively accessed"),
            TAKEN => write!(f, "moved"),
            n if n < 0 => write!(f, "shared by {}", -n),
            n => write!(f, "invalidly marked ({})", n),
        }
    }
}

#[derive(Clone)]
pub(crate) struct Access(Cell<isize>);

impl Access {
    /// Construct a new default access.
    pub(crate) const fn new() -> Self {
        Self(Cell::new(0))
    }

    /// Test if we have shared access without modifying the internal count.
    #[inline]
    pub(crate) fn is_shared(&self) -> bool {
        self.0.get().wrapping_sub(1) < 0
    }

    /// Test if we have exclusive access without modifying the internal count.
    #[inline]
    pub(crate) fn is_exclusive(&self) -> bool {
        self.0.get() == 0
    }

    /// Test if the data has been taken.
    #[inline]
    pub(crate) fn is_taken(&self) -> bool {
        self.0.get() == isize::max_value()
    }

    /// Mark that we want shared access to the given access token.
    #[inline]
    pub(crate) fn shared(&self) -> Result<RawBorrowedRef, NotAccessibleRef> {
        let state = self.0.get();
        let n = state.wrapping_sub(1);

        if n >= 0 {
            return Err(NotAccessibleRef(Snapshot(state)));
        }

        self.0.set(n);
        Ok(RawBorrowedRef { access: self })
    }

    /// Mark that we want exclusive access to the given access token.
    #[inline]
    pub(crate) fn exclusive(&self) -> Result<RawBorrowedMut, NotAccessibleMut> {
        let state = self.0.get();
        let n = state.wrapping_add(1);

        if n != 1 {
            return Err(NotAccessibleMut(Snapshot(state)));
        }

        self.0.set(n);
        Ok(RawBorrowedMut { access: self })
    }

    /// Mark that we want to mark the given access as "taken".
    ///
    /// I.e. whatever guarded data is no longer available.
    #[inline]
    pub(crate) fn take(&self) -> Result<RawTakeGuard, NotAccessibleTake> {
        let state = self.0.get();

        if state != 0 {
            return Err(NotAccessibleTake(Snapshot(state)));
        }

        self.0.set(isize::max_value());
        Ok(RawTakeGuard { access: self })
    }

    /// Unshare the current access.
    #[inline]
    fn release_shared(&self) {
        let b = self.0.get().wrapping_add(1);
        debug_assert!(b <= 0);
        self.0.set(b);
    }

    /// Unshare the current access.
    #[inline]
    fn release_exclusive(&self) {
        let b = self.0.get().wrapping_sub(1);
        debug_assert!(b == 0);
        self.0.set(b);
    }

    /// Unshare the current access.
    #[inline]
    fn release_take(&self) {
        let b = self.0.get();
        debug_assert!(b == isize::max_value());
        self.0.set(0);
    }
}

impl fmt::Debug for Access {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Snapshot(self.0.get()))
    }
}

/// A raw reference guard.
pub struct RawBorrowedRef {
    access: *const Access,
}

impl Drop for RawBorrowedRef {
    fn drop(&mut self) {
        unsafe { (*self.access).release_shared() };
    }
}

/// Guard for a data borrowed from a slot in the virtual machine.
///
/// These guards are necessary, since we need to guarantee certain forms of
/// access depending on what we do. Releasing the guard releases the access.
pub struct BorrowRef<'a, T: ?Sized + 'a> {
    data: *const T,
    guard: RawBorrowedRef,
    _marker: marker::PhantomData<&'a T>,
}

impl<'a, T: ?Sized> BorrowRef<'a, T> {
    /// Construct a new raw reference guard.
    ///
    /// # Safety
    ///
    /// The provided components must be valid for the lifetime of the returned
    /// reference, which is unbounded.
    pub(crate) unsafe fn from_raw(data: *const T, guard: RawBorrowedRef) -> Self {
        Self {
            data,
            guard,
            _marker: marker::PhantomData,
        }
    }

    /// Try to map the interior reference the reference.
    pub fn try_map<M, U: ?Sized, E>(this: Self, m: M) -> Result<BorrowRef<'a, U>, E>
    where
        M: FnOnce(&T) -> Result<&U, E>,
    {
        let data = m(unsafe { &*this.data })?;
        let guard = this.guard;

        Ok(BorrowRef {
            data,
            guard,
            _marker: marker::PhantomData,
        })
    }
}

impl<T: ?Sized> ops::Deref for BorrowRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data }
    }
}

impl<T: ?Sized> fmt::Debug for BorrowRef<'_, T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, fmt)
    }
}

/// A raw mutable guard.
pub struct RawBorrowedMut {
    access: *const Access,
}

impl Drop for RawBorrowedMut {
    fn drop(&mut self) {
        unsafe { (*self.access).release_exclusive() }
    }
}

/// A raw take guard.
///
/// Dropping this will undo the take operation.
pub(crate) struct RawTakeGuard {
    access: *const Access,
}

impl Drop for RawTakeGuard {
    fn drop(&mut self) {
        unsafe { (*self.access).release_take() }
    }
}

/// Guard for data exclusively borrowed from a slot in the virtual machine.
///
/// These guards are necessary, since we need to guarantee certain forms of
/// access depending on what we do. Releasing the guard releases the access.
pub struct BorrowMut<'a, T: ?Sized> {
    data: *mut T,
    guard: RawBorrowedMut,
    _marker: marker::PhantomData<&'a mut T>,
}

impl<'a, T: ?Sized> BorrowMut<'a, T> {
    /// Construct a new raw reference guard.
    ///
    /// # Safety
    ///
    /// The provided components must be valid for the lifetime of the returned
    /// reference, which is unbounded.
    pub(crate) unsafe fn from_raw(data: *mut T, guard: RawBorrowedMut) -> Self {
        Self {
            data,
            guard,
            _marker: marker::PhantomData,
        }
    }

    /// Map the mutable reference.
    pub fn try_map<M, U: ?Sized, E>(this: Self, m: M) -> Result<BorrowMut<'a, U>, E>
    where
        M: FnOnce(&mut T) -> Result<&mut U, E>,
    {
        let data = m(unsafe { &mut *this.data })?;
        let guard = this.guard;

        Ok(BorrowMut {
            data,
            guard,
            _marker: marker::PhantomData,
        })
    }
}

impl<T: ?Sized> ops::Deref for BorrowMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.data }
    }
}

impl<T: ?Sized> ops::DerefMut for BorrowMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.data }
    }
}

impl<T: ?Sized> fmt::Debug for BorrowMut<'_, T>
where
    T: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, fmt)
    }
}

impl<F> Future for BorrowMut<'_, F>
where
    F: Unpin + Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // NB: inner Future is Unpin.
        let this = self.get_mut();
        Pin::new(&mut **this).poll(cx)
    }
}
