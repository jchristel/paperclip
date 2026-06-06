// crates/aconex/src/lib.rs
//
// Library crate for talking to the Aconex API.
// `lib.rs` is the crate root for a library — the equivalent of `main.rs`
// for a binary. Whatever is marked `pub` here is what other crates (like
// paperclip-cli) can see, similar to `public` types in a C# class library.

// A placeholder so the crate compiles with no warnings about being empty.
// We'll replace this with real modules (client, auth, ...) next time.
#[cfg(test)]
mod tests {
    #[test]
    fn it_builds() {
        // Proves the crate compiles and the test harness runs.
        assert_eq!(2 + 2, 4);
    }
}