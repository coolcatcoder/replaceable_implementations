use counters::Counters;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use std::env::VarError;
use syn::{Ident, ItemImpl, Path};

mod counters;

static IMPLEMENTATION_COUNTERS: Counters = Counters::new();

/// Due to orphan rules, we need to perform some setup.  
/// This should be put in one spot that you know the path to.
/// That could be crate root, or a specific module that is passed into your macros. It is up to you.  
/// The capacity is used to set the max number of implementations that can be replaced.
pub fn setup(capacity: u32) -> TokenStream {
    let mut output = quote! {
        /// Self is the same type as T.
        /// Used to bypass trivial bounds.
        #[doc(hidden)]
        pub trait Is<T> {}
        impl<T> Is<T> for T {}
    };

    (0..capacity).for_each(|index| {
        let ident = Ident::new(&format!("Switch{index}"), Span::call_site());
        output.extend(quote! {
            #[doc(hidden)]
            pub struct #ident<T, const BOOL: bool>(core::marker::PhantomData<T>);
        });
    });
    output
}

pub fn replace(path_to_setup: Path, id: String, with: impl FnOnce(u16) -> ItemImpl) -> TokenStream {
    let replaced = match IMPLEMENTATION_COUNTERS.fetch_add(id, 0) {
        Ok(replaced) => replaced,
        Err(VarError::NotPresent) => {
            return syn::Error::new(
                Span::call_site(),
                "The crate name was not present in the environment variables.",
            )
            .into_compile_error();
        }
        Err(VarError::NotUnicode(crate_name)) => {
            return syn::Error::new(
                Span::call_site(),
                format!(
                    "The crate name was not unicode. Crate name: {:?}",
                    crate_name
                ),
            )
            .into_compile_error();
        }
    };

    let mut implementation = with(replaced);

    let switch_current = Ident::new(&format!("Switch{}", replaced), Span::call_site());

    let kidnapped_type = &*implementation.self_ty;
    let kidnapped = match syn::parse2(quote! {
        Kidnapped: #path_to_setup::Is<#kidnapped_type>
    }) {
        Ok(kidnapped) => kidnapped,
        Err(error) => return error.to_compile_error(),
    };

    implementation
        .generics
        .params
        .push(syn::GenericParam::Type(kidnapped));

    *implementation.self_ty = syn::parse2(quote! {Kidnapped}).unwrap();

    let predicate = match syn::parse2(
        quote! {#path_to_setup::#switch_current<Kidnapped, true>: core::marker::Unpin},
    ) {
        Ok(predicate) => predicate,
        Err(error) => return error.to_compile_error(),
    };
    implementation
        .generics
        .make_where_clause()
        .predicates
        .push(predicate);

    let output = if replaced == 0 {
        implementation.to_token_stream()
    } else {
        let switch_previous = Ident::new(&format!("Switch{}", replaced - 1), Span::call_site());
        quote! {
            #implementation
            impl<T> core::marker::Unpin for #path_to_setup::#switch_previous<T, false> {}
        }
    };

    output
}
