#![expect(missing_docs, clippy::dbg_macro, unused_variables, reason = "TODO")]

use proc_macro::TokenStream;

#[proc_macro_attribute]
#[inline]
pub fn disguise_original(args: TokenStream, input: TokenStream) -> TokenStream {
    dbg!(&input);
    input
}

#[proc_macro_attribute]
#[inline]
pub fn disguise_with(args: TokenStream, input: TokenStream) -> TokenStream {
    dbg!(&input);
    input
}
