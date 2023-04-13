#![no_std]
// lint me harder
#![forbid(non_ascii_idents)]
#![deny(
    future_incompatible,
    keyword_idents,
    elided_lifetimes_in_paths,
    meta_variable_misuse,
    noop_method_call,
    pointer_structural_match,
    unused_lifetimes,
    unused_qualifications,
    unsafe_op_in_unsafe_fn,
    clippy::undocumented_unsafe_blocks,
    clippy::wildcard_dependencies,
    clippy::debug_assert_with_mut_call,
    clippy::empty_line_after_outer_attr,
    clippy::panic,
    clippy::unwrap_used,
    clippy::redundant_field_names,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::unneeded_field_pattern,
    clippy::useless_let_if_seq
)]
#![warn(clippy::pedantic, missing_docs)]

//! This crate is intended to solve one very specific problem: returning a generic value from an
//! object-safe trait.
//!
//! ```rust
//! # use core::str::FromStr;
//! # use dyngo::{Proof, SafeSlot};
//! // Let's say you have an object-safe interface for providing a string
//! trait StringProviderBad {
//!     fn provide(&self, f: &mut dyn FnMut(&str));
//! }
//!
//! // You can't just return `&str`, because it can refer to a local value inside of a method:
//! struct TwoParts(&'static str, &'static str);
//!
//! impl StringProviderBad for TwoParts {
//!     fn provide(&self, f: &mut dyn FnMut(&str)) {
//!         f(&format!("{}{}", self.0, self.1))
//!     }
//! }
//!
//! // Let's try to use this interface:
//! fn parse_provided_string_bad<T: FromStr>(provider: &dyn StringProviderBad) -> Option<T> {
//!     provider.provide(&mut |s| {
//!         let parsed = T::from_str(s).ok();
//!         // But how to actually return it?
//!     });
//!     todo!()
//! }
//!
//! // dyngo provides a type-safe solution to this problem:
//! trait StringProvider {
//!     fn provide<'id>(&self, f: &mut dyn FnMut(&str) -> Proof<'id>) -> Proof<'id>;
//!     //                                                ^^^^^^^^^^     ^^^^^^^^^^
//!     // new: now `.provide()` returns a `Proof` that `f` was called
//! }
//!
//! // Implementation is just about the same:
//! impl StringProvider for TwoParts {
//!     fn provide<'id>(&self, f: &mut dyn FnMut(&str) -> Proof<'id>) -> Proof<'id> {
//!         f(&format!("{}{}", self.0, self.1))
//!     }
//! }
//!
//! // And now we can use the interface to return a generic value from the provider:
//! fn parse_provided_string<T: FromStr>(provider: &dyn StringProvider) -> Option<T> {
//!     SafeSlot::with(|mut slot| {
//!         let proof = provider.provide(&mut |s| slot.fill(T::from_str(s).ok()));
//!         slot.unlock(proof)
//!     })
//! }
//!
//! let num = parse_provided_string::<i32>(&TwoParts("4", "2"));
//! assert_eq!(num, Some(42));
//! ```
//!
//! Note that trying to use a wrong [`Proof`] for a [`Slot`] fails in compile time: both
//!
//! ```rust,compile_fail
//! SafeSlot::with(|mut slot1: SafeSlot<i32>| {
//!     SafeSlot::with(|mut slot2: SafeSlot<i32>| {
//!         let proof1 = slot1.write(42);
//!         slot2.unlock(proof1);
//!     })
//! })
//! ```
//!
//! and
//!
//! ```rust,compile_fail
//! SafeSlot::with(|mut slot1: SafeSlot<i32>| {
//!     SafeSlot::with(|mut slot2: SafeSlot<i32>| {
//!         let proof2 = slot2.write(42);
//!         slot1.unlock(proof2);
//!     })
//! })
//! ```
//!
//! fail to compile.

use core::{marker::PhantomData, mem::MaybeUninit};

struct Invariant<'id>(PhantomData<fn(&'id ()) -> &'id ()>);

impl Invariant<'_> {
    const LT: Self = Self(PhantomData);
}

/// Slot on stack to place values into.
///
/// You probably should use either [`SafeSlot`] or [`LeakySlot`].
pub struct Slot<'id, T, C>
where
    C: Container<T>,
{
    contents: C,
    _value: PhantomData<T>,
    _lifetime: Invariant<'id>,
}

/// A completely safe (no unsafe code) [`Slot`] that never leaks memory unless it's leaked.
///
/// It's not competely zero cost: it's size is greater than the size of `T` and
/// [`.unlock()`](Self::unlock) contains a conditional branch.
pub type SafeSlot<'id, T> = Slot<'id, T, Option<T>>;

/// A [`MaybeUninit<_>`] based [`Slot`] that's zero cost, but leaks memory if value inside is not
/// consumed.
///
/// In particular, a contained value is leaked in either of two scenarios:
/// 1. Two calls to [`.fill()`](Self::fill) occur to the same slot.
/// 2. A call to [`.fill()`](Self::fill) occurs without a call to [`.unlock()`](Self::unlock)
///    later.
pub type LeakySlot<'id, T> = Slot<'id, T, MaybeUninit<T>>;

/// Proof that [`Slot`] was successfully initialized.
///
/// Pass it to [`.unlock()`](Slot::unlock) to get the contained value.
pub struct Proof<'id>(Invariant<'id>);

impl<T, C> Slot<'_, T, C>
where
    C: Container<T>,
{
    /// Create a new [`Slot`], passing it to the provided function.
    pub fn with<R>(f: impl for<'id> FnOnce(Slot<'id, T, C>) -> R) -> R {
        f(Slot {
            contents: C::empty(),
            _value: PhantomData,
            _lifetime: Invariant::LT,
        })
    }
}

impl<'id, C, T> Slot<'id, T, C>
where
    C: Container<T>,
{
    /// Place a value into the [`Slot`], returning a [`Proof`] that can be used to later retrieve
    /// it by calling [`.unlock()`](Self::unlock).
    pub fn fill(&mut self, val: T) -> Proof<'id> {
        self.contents.fill(val);
        Proof(Invariant::LT)
    }

    /// Get the contained value from this [`Slot`].
    ///
    /// You need to pass a [`Proof`] that was previously produced by a call to
    /// [`.fill()`](Self::fill) on the same [`Slot`].
    ///
    /// Trying to pass [`Proof`] from the wrong [`Slot`] will result in a compilation error.
    #[allow(clippy::needless_pass_by_value)]
    pub fn unlock(self, _proof: Proof<'id>) -> T {
        // SAFETY: we have a `Proof` that write previously occured
        unsafe { self.contents.unpack() }
    }
}

/// Entity that could be used for storage of one element of type `T`.
///
/// # Safety
/// It must be safe to call [`.unpack()`](Self::unpack) after [`.fill()`](Self::fill).
///
/// Dropping filled container or calling [`.fill()`](Self::fill) twice may leak memory.
pub unsafe trait Container<T> {
    /// Create an empty [`Container`].
    fn empty() -> Self;

    /// Place `val` into the container.
    fn fill(&mut self, val: T);

    /// Take value from the container.
    ///
    /// # Safety
    /// [`.fill()`](Self::fill) must be called first.
    unsafe fn unpack(self) -> T;
}

// SAFETY: `.unpack()` is always safe here
unsafe impl<T> Container<T> for Option<T> {
    fn empty() -> Self {
        None
    }

    fn fill(&mut self, val: T) {
        *self = Some(val);
    }

    unsafe fn unpack(self) -> T {
        self.expect("trying to unpack None Container")
    }
}

// SAFETY: `.unpack()` is safe after `.fill()` because `.assume_init()` is safe after `.write()`
unsafe impl<T> Container<T> for MaybeUninit<T> {
    fn empty() -> Self {
        MaybeUninit::uninit()
    }

    fn fill(&mut self, val: T) {
        self.write(val);
    }

    unsafe fn unpack(self) -> T {
        // SAFETY: guaranteed by the caller
        unsafe { self.assume_init() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn test_generic<C: Container<i32>>() {
        assert_eq!(
            Slot::<i32, C>::with(|mut slot| {
                let proof = slot.fill(42);
                slot.unlock(proof)
            }),
            42,
        );
    }

    #[test]
    fn safe() {
        test_generic::<Option<i32>>();
    }

    #[test]
    fn leaky() {
        test_generic::<MaybeUninit<i32>>();
    }

    #[test]
    fn leaky_is_free() {
        assert_eq!(core::mem::size_of::<LeakySlot<'_, u64>>(), 8);
    }

    #[test]
    fn safe_doesnt_leak() {
        use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

        struct ObservableDrop<'a>(&'a AtomicUsize);

        impl Drop for ObservableDrop<'_> {
            fn drop(&mut self) {
                self.0.fetch_add(1, Relaxed);
            }
        }

        let drop_count = AtomicUsize::new(0);
        SafeSlot::with(|mut slot| {
            slot.fill(ObservableDrop(&drop_count));
            assert_eq!(drop_count.load(Relaxed), 0);
            slot.fill(ObservableDrop(&drop_count));
            assert_eq!(drop_count.load(Relaxed), 1);
        });
        assert_eq!(drop_count.load(Relaxed), 2);
    }
}
