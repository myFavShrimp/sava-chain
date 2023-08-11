use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::{
    bracketed, parenthesized, parse::Parse, parse_macro_input, token::Paren, ExprClosure, Ident,
    Token,
};

struct ChainingValidator {
    extractor_fn: ExprClosure,
    combinator_fn: ExprClosure,
    validator: Ident,
}

impl Parse for ChainingValidator {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let inner;
        parenthesized!(inner in input);

        let extractor_fn: ExprClosure = inner.parse()?;
        inner.parse::<Token![,]>()?;
        let combinator_fn: ExprClosure = inner.parse()?;
        inner.parse::<Token![,]>()?;
        let validator: Ident = inner.parse()?;

        if let (Err(e), true) = (input.parse::<Token![,]>(), input.peek(Paren)) {
            return Err(e);
        }

        Ok(Self {
            extractor_fn,
            combinator_fn,
            validator,
        })
    }
}

impl ChainingValidator {
    pub fn execute_part(&self, to_validate: &Ident) -> TokenStream2 {
        let ChainingValidator {
            validator,
            extractor_fn,
            combinator_fn,
        } = self;

        quote::quote! {
            let extractor_fn: ::sava_chain::FieldExtractorFn<#to_validate, <#validator as ::sava_chain::ChainExec>::Type> = #extractor_fn;
            let combinator_fn: ::sava_chain::FieldCombinatorFn<<#validator as ::sava_chain::ChainExec>::Type, #to_validate> = #combinator_fn;

            let extracted_field = extractor_fn(&data);
            let chain_result = #validator::execute(extracted_field)?;
            combinator_fn(&mut data, chain_result);
        }
    }
}

struct Chaining {
    to_validate: Ident,
    error: Ident,
    name: Ident,
    validators: Vec<ChainingValidator>,
}

impl Parse for Chaining {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut validators = Vec::new();

        let (to_validate, error) = {
            let to_validate_error_pair;
            parenthesized!(to_validate_error_pair in input);

            let to_validate: Ident = to_validate_error_pair.parse()?;
            to_validate_error_pair.parse::<Token![,]>()?;
            let error: Ident = to_validate_error_pair.parse()?;

            (to_validate, error)
        };

        input.parse::<Token![=>]>()?;
        let name: Ident = input.parse()?;
        input.parse::<Token![:]>()?;

        let inner;
        bracketed!(inner in input);

        while !inner.is_empty() {
            validators.push(inner.parse()?)
        }

        if let (Err(e), true) = (input.parse::<Token![,]>(), input.peek(Paren)) {
            return Err(e);
        }

        Ok(Self {
            to_validate,
            error,
            name,
            validators,
        })
    }
}

impl Chaining {
    pub fn chain_exec(&self) -> TokenStream2 {
        let Chaining {
            to_validate,
            error,
            name,
            validators,
        } = self;
        let validation = self.validate();

        let mut execute = Vec::new();

        for validator in validators {
            execute.push(validator.execute_part(&to_validate));
        }

        quote::quote! {
            pub struct #name;
            impl ::sava_chain::ChainExec for #name {
                type Type = #to_validate;
                type Error = #error;

                fn execute(input: Self::Type) -> Result<Self::Type, Self::Error> {
                    #validation

                    let mut data = input;

                    #(#execute)*

                    Ok(data)
                }
            }
        }
    }

    pub fn validate(&self) -> TokenStream2 {
        let Chaining {
            to_validate: _,
            error,
            name: _,
            validators,
        } = self;

        let validator_idents: Vec<&Ident> = validators
            .iter()
            .map(|validator| &validator.validator)
            .collect();

        let assert_error = quote::quote_spanned! {
            error.span() => struct _AssertError where #error: std::error::Error;
        };

        let assert_error_from = quote::quote_spanned! {
            error.span() => struct _AssertErrorFrom
            where
                #error: #(
                    std::convert::From<<#validator_idents as ::sava_chain::ChainExec>::Error>
                )+*;
        };

        quote::quote! {
            #assert_error
            #assert_error_from
        }
    }
}

struct Chainings(Vec<Chaining>);

impl Parse for Chainings {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let mut chainings = Vec::new();

        while !input.is_empty() {
            chainings.push(input.parse()?)
        }

        Ok(Self(chainings))
    }
}

#[proc_macro]
pub fn chaining(input: TokenStream) -> TokenStream {
    let Chainings(chainings) = parse_macro_input!(input as Chainings);

    let mut result = TokenStream2::new();

    for chaining in chainings {
        result.extend(chaining.chain_exec());
    }

    result.into()
}
