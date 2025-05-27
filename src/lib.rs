#![deny(clippy::unwrap_used)]

use counters::Counters;
use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use std::env::VarError;
use syn::{Ident, ItemImpl, Path};

mod counters;

static IMPLEMENTATION_COUNTERS: Counters = Counters::new();

/// Due to orphan rules, we need to perform some setup.\
/// This should be put in one spot that you know the path to.
/// That could be crate root, or a specific module that is passed into your macros. It is up to you.\
/// The capacity is used to set the max number of implementations that can be replaced.
#[must_use]
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

/// Provides an initial implementation.\
/// Any other implementations will replace this one, no matter the execution order.
/// 
/// # Errors
/// It will only error if there is something wrong with the `path_to_setup` or the `implementation`.
pub fn initial_implementation(
    path_to_setup: &Path,
    implementation: ItemImpl,
) -> Result<ItemImpl, syn::Error> {
    make_replaceable(path_to_setup, 0, implementation)
}

/// Replaces the previous implementation marked by the `id` with a new implementation.\
/// Returns the quantity of previous implementations as well as the function that will actually replace the implementation.
/// 
/// # Be Careful
/// There is no guarantee on the execution order of procedural macros.\
/// You must take care to make sure that your macros work in all execution orders.
/// 
/// # Errors
/// Will only error if the crate name cannot be fetched from the environment variables.
pub fn replace_implementation(
    path_to_setup: &Path,
    id: String,
    has_initial_implementation: bool,
) -> Result<(u16, impl FnOnce(ItemImpl) -> TokenStream), VarError> {
    let previous_implementations =
        IMPLEMENTATION_COUNTERS.fetch_add(id, has_initial_implementation.into())?;
    let replace = move |implementation: ItemImpl| -> TokenStream {
        let implementation =
            match make_replaceable(path_to_setup, previous_implementations, implementation) {
                Ok(implementation) => implementation,
                Err(error) => return error.to_compile_error(),
            };

        // If there are no previous implementations, then we don't need to replace anything.
        if previous_implementations == 0 {
            implementation.to_token_stream()
        } else {
            let switch_previous = Ident::new(
                &format!("Switch{}", previous_implementations - 1),
                Span::call_site(),
            );
            quote! {
                #implementation
                impl<T> core::marker::Unpin for #path_to_setup::#switch_previous<T, false> {}
            }
        }
    };
    Ok((previous_implementations, replace))
}

fn make_replaceable(
    path_to_setup: &Path,
    previous_implementations: u16,
    mut implementation: ItemImpl,
) -> Result<ItemImpl, syn::Error> {
    // Current is the previous value because the switch index starts from 0.
    let switch_current = Ident::new(
        &format!("Switch{previous_implementations}"),
        Span::call_site(),
    );

    let kidnapped_type = &*implementation.self_ty;
    let kidnapped = syn::parse2(quote! {
        Kidnapped: #path_to_setup::Is<#kidnapped_type>
    })?;

    implementation
        .generics
        .params
        .push(syn::GenericParam::Type(kidnapped));

    *implementation.self_ty = syn::parse2(quote! {Kidnapped})?;

    let predicate = syn::parse2(
        quote! {#path_to_setup::#switch_current<Kidnapped, true>: core::marker::Unpin},
    )?;
    implementation
        .generics
        .make_where_clause()
        .predicates
        .push(predicate);

    Ok(implementation)
}
