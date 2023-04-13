# dyngo: dynamic generic outparams

This crate is intended to solve one very specific problem: returning a generic value from an
object-safe trait.

```rust
// Let's say you have an object-safe interface for providing a string
trait StringProviderBad {
    fn provide(&self, f: &mut dyn FnMut(&str));
}

// You can't just return `&str`, because it can refer to a local value inside of a method:
struct TwoParts(&'static str, &'static str);

impl StringProviderBad for TwoParts {
    fn provide(&self, f: &mut dyn FnMut(&str)) {
        f(&format!("{}{}", self.0, self.1))
    }
}

// Let's try to use this interface:
fn parse_provided_string_bad<T: FromStr>(provider: &dyn StringProviderBad) -> Option<T> {
    provider.provide(&mut |s| {
        let parsed = T::from_str(s).ok();
        // But how to actually return it?
    });
    todo!()
}

// dyngo provides a type-safe solution to this problem:
trait StringProvider {
    fn provide<'id>(&self, f: &mut dyn FnMut(&str) -> Proof<'id>) -> Proof<'id>;
    //                                                ^^^^^^^^^^     ^^^^^^^^^^
    // new: now `.provide()` returns a `Proof` that `f` was called
}

// Implementation is just about the same:
impl StringProvider for TwoParts {
    fn provide<'id>(&self, f: &mut dyn FnMut(&str) -> Proof<'id>) -> Proof<'id> {
        f(&format!("{}{}", self.0, self.1))
    }
}

// And now we can use the interface to return a generic value from the provider:
fn parse_provided_string<T: FromStr>(provider: &dyn StringProvider) -> Option<T> {
    SafeSlot::with(|mut slot| {
        let proof = provider.provide(&mut |s| slot.fill(T::from_str(s).ok()));
        slot.unlock(proof)
    })
}

let num = parse_provided_string::<i32>(&TwoParts("4", "2"));
assert_eq!(num, Some(42));
```

Note that trying to use a wrong `Proof` for a `Slot` fails in compile time: both

```rust,compile_fail
SafeSlot::with(|mut slot1: SafeSlot<i32>| {
    SafeSlot::with(|mut slot2: SafeSlot<i32>| {
        let proof1 = slot1.write(42);
        slot2.unlock(proof1);
    })
})
```

and

```rust,compile_fail
SafeSlot::with(|mut slot1: SafeSlot<i32>| {
    SafeSlot::with(|mut slot2: SafeSlot<i32>| {
        let proof2 = slot2.write(42);
        slot1.unlock(proof2);
    })
})
```

fail to compile.
