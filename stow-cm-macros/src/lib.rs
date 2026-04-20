extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::{Data, DeriveInput, Fields, parse_macro_input};

/// Derive macro for the `Finalize` trait.
///
/// Generates an impl that calls `.finalize()` on every field of the struct.
/// Fields annotated with `#[finalize(skip)]` are excluded.
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Finalize)]
/// pub struct Config {
///     pub target: Option<PathBuf>,
///     pub ignore: Option<Vec<String>>,
///     #[finalize(skip)]
///     pub fold: Option<bool>,
/// }
/// // expands to:
/// // impl Finalize for Config {
/// //     fn finalize(&mut self) {
/// //         self.target.finalize();
/// //         self.ignore.finalize();
/// //     }
/// // }
/// ```
#[proc_macro_derive(Finalize, attributes(finalize))]
pub fn derive_finalize(input: TokenStream) -> TokenStream {
    let ast = parse_macro_input!(input as DeriveInput);
    impl_finalize(&ast)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

fn impl_finalize(ast: &DeriveInput) -> syn::Result<proc_macro2::TokenStream> {
    let name = &ast.ident;

    let fields = match &ast.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(named) => &named.named,
            _ => {
                return Err(syn::Error::new(
                    Span::call_site(),
                    "Finalize can only be derived for structs with named fields",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new(
                Span::call_site(),
                "Finalize can only be derived for structs",
            ));
        }
    };

    let calls = fields.iter().filter(|f| !has_skip_attr(f)).map(|f| {
        let ident = f.ident.as_ref().expect("named field has ident");
        quote! { self.#ident.finalize(); }
    });

    Ok(quote! {
        impl crate::merge::Finalize for #name {
            fn finalize(&mut self) {
                #( #calls )*
            }
        }
    })
}

/// Returns true if the field has `#[finalize(skip)]`.
fn has_skip_attr(field: &syn::Field) -> bool {
    field.attrs.iter().any(|attr| {
        if !attr.path().is_ident("finalize") {
            return false;
        }
        let mut found_skip = false;
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                found_skip = true;
            }
            Ok(())
        });
        found_skip
    })
}
