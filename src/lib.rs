#![deny(clippy::unwrap_used)]
#![warn(clippy::pedantic)]

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

pub fn initial_implementation(
    path_to_setup: &Path,
    implementation: ItemImpl,
) -> Result<ItemImpl, syn::Error> {
    make_replaceable(path_to_setup, 0, implementation)
}

pub fn previous_implementations_and_replace(
    path_to_setup: &Path,
    id: String,
    has_initial_implementation: bool,
) -> Result<(u16, impl FnOnce(ItemImpl) -> TokenStream), VarError> {
    let previous_implementations =
        IMPLEMENTATION_COUNTERS.fetch_add(id, has_initial_implementation as u16)?;
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

pub fn replace<E>(
    path_to_setup: &Path,
    id: String,
    with: impl FnOnce(u16) -> Result<ItemImpl, E>,
) -> Result<TokenStream, E> {
    let previous_implementations = match IMPLEMENTATION_COUNTERS.fetch_add(id, 0) {
        Ok(previous_implementations) => previous_implementations,
        Err(VarError::NotPresent) => {
            return Ok(syn::Error::new(
                Span::call_site(),
                "The crate name was not present in the environment variables.",
            )
            .into_compile_error());
        }
        Err(VarError::NotUnicode(crate_name)) => {
            return Ok(syn::Error::new(
                Span::call_site(),
                format!(
                    "The crate name was not unicode. Crate name: {:?}",
                    crate_name
                ),
            )
            .into_compile_error());
        }
    };

    let implementation = match make_replaceable(
        path_to_setup,
        previous_implementations,
        with(previous_implementations)?,
    ) {
        Ok(implementation) => implementation,
        Err(error) => return Ok(error.to_compile_error()),
    };

    // If there are no previous implementations, then we don't need to replace anything.
    let output = if previous_implementations == 0 {
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
    };

    Ok(output)
}

fn make_replaceable(
    path_to_setup: &Path,
    previous_implementations: u16,
    mut implementation: ItemImpl,
) -> Result<ItemImpl, syn::Error> {
    // Current is the previous value because the switch index starts from 0.
    let switch_current = Ident::new(
        &format!("Switch{}", previous_implementations),
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
