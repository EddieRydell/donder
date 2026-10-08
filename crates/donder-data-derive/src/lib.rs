//! `#[derive(Data)]` for Donder project-language document types.
//!
//! - A struct with named fields is a record: `Name { field: value, ... }`,
//!   every field written, in declaration order. It also implements `Record`,
//!   so it can be a declaration's body.
//! - An enum is a choice of variants: a unit variant is its bare name, and a
//!   variant with named fields is written like a record.
//!
//! The Rust names are the language names, so a type's definition is its
//! schema.
use proc_macro::TokenStream;
use proc_macro2::TokenStream as Tokens;
use quote::quote;
use syn::{Data, DeriveInput, Fields, FieldsNamed, Ident, parse_macro_input};

#[proc_macro_derive(Data)]
pub fn derive_data(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let output = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => record(&input.ident, fields),
            _ => error(&input.ident, "a record needs named fields"),
        },
        Data::Enum(data) => {
            let mut variants = Vec::new();
            for variant in &data.variants {
                match &variant.fields {
                    Fields::Unit => variants.push((&variant.ident, None)),
                    Fields::Named(fields) => variants.push((&variant.ident, Some(fields))),
                    Fields::Unnamed(_) => {
                        return error(&variant.ident, "a variant is bare or has named fields")
                            .into();
                    }
                }
            }
            choice(&input.ident, &variants)
        }
        Data::Union(_) => error(&input.ident, "unions are not data"),
    };
    output.into()
}

fn error(ident: &Ident, message: &str) -> Tokens {
    syn::Error::new(ident.span(), message).to_compile_error()
}

fn schema() -> Tokens {
    quote!(::donder_language::data)
}

/// Statements reading each field from `reader`, then the struct-literal body.
fn read_fields(fields: &FieldsNamed) -> (Tokens, Tokens) {
    let names: Vec<_> = fields
        .named
        .iter()
        .map(|field| field.ident.clone().unwrap_or_else(|| unreachable!("named")))
        .collect();
    let types: Vec<_> = fields.named.iter().map(|field| &field.ty).collect();
    let texts: Vec<_> = names.iter().map(|name| name.to_string()).collect();
    let reads = quote! {
        #(let #names = reader.field::<#types>(#texts, decoder);)*
    };
    let build = quote! { #(#names: #names?,)* };
    (reads, build)
}

fn write_fields(fields: &FieldsNamed, access: impl Fn(&Ident) -> Tokens) -> Tokens {
    let schema = schema();
    let writes = fields.named.iter().map(|field| {
        let name = field.ident.clone().unwrap_or_else(|| unreachable!("named"));
        let text = name.to_string();
        let value = access(&name);
        quote!(#schema::field(#text, #schema::Data::encode(#value)))
    });
    quote!(vec![#(#writes),*])
}

fn field_shapes(fields: &FieldsNamed) -> Tokens {
    let schema = schema();
    let shapes = fields.named.iter().map(|field| {
        let text = field
            .ident
            .as_ref()
            .unwrap_or_else(|| unreachable!("named"))
            .to_string();
        let ty = &field.ty;
        quote!((#text, <#ty as #schema::Data>::shape(schema)))
    });
    quote!(vec![#(#shapes),*])
}

fn record(ident: &Ident, fields: &FieldsNamed) -> Tokens {
    let schema = schema();
    let text = ident.to_string();
    let (reads, build) = read_fields(fields);
    let writes = write_fields(fields, |name| quote!(&self.#name));
    let shapes = field_shapes(fields);
    quote! {
        impl #schema::Record for #ident {
            const TYPE: &'static str = #text;
            fn decode_fields(
                fields: &::donder_language::data::Spanned<
                    ::std::vec::Vec<::donder_language::data::DataField>,
                >,
                decoder: &mut #schema::Decoder,
            ) -> ::core::option::Option<Self> {
                let mut reader = #schema::FieldReader::new(#text, fields);
                #reads
                reader.finish(decoder)?;
                ::core::option::Option::Some(Self { #build })
            }
            fn encode_fields(&self) -> ::std::vec::Vec<::donder_language::data::DataField> {
                #writes
            }
        }
        impl #schema::Data for #ident {
            fn decode(
                value: &::donder_language::data::Spanned<::donder_language::data::DataValue>,
                decoder: &mut #schema::Decoder,
            ) -> ::core::option::Option<Self> {
                #schema::decode_record(value, decoder)
            }
            fn encode(&self) -> ::donder_language::data::DataValue {
                #schema::record(#text, #schema::Record::encode_fields(self))
            }
            fn shape(schema: &mut #schema::Schema) -> #schema::Shape {
                schema.named(#text, |schema| #schema::Definition::Record(#shapes))
            }
        }
    }
}

fn choice(ident: &Ident, variants: &[(&Ident, Option<&FieldsNamed>)]) -> Tokens {
    let schema = schema();
    let text = ident.to_string();
    let expected = variants
        .iter()
        .map(|(name, fields)| match fields {
            None => format!("`{name}`"),
            Some(_) => format!("`{name} {{ ... }}`"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let expected = format!("one of {expected}");
    let bare = variants
        .iter()
        .filter(|(_, fields)| fields.is_none())
        .map(|(name, _)| {
            let text = name.to_string();
            quote!(#text => ::core::option::Option::Some(Self::#name))
        });
    let records = variants.iter().filter_map(|(name, fields)| {
        let fields = (*fields)?;
        let text = name.to_string();
        let (reads, build) = read_fields(fields);
        Some(quote! {
            #text => {
                let mut reader = #schema::FieldReader::new(#text, fields);
                #reads
                reader.finish(decoder)?;
                ::core::option::Option::Some(Self::#name { #build })
            }
        })
    });
    let encodes = variants.iter().map(|(name, fields)| {
        let text = name.to_string();
        match fields {
            None => quote!(Self::#name => #schema::variant(#text)),
            Some(fields) => {
                let bindings = fields.named.iter().map(|field| &field.ident);
                let writes = write_fields(fields, |name| quote!(#name));
                quote!(Self::#name { #(#bindings),* } => #schema::record(#text, #writes))
            }
        }
    });
    let shapes = variants.iter().map(|(name, fields)| {
        let text = name.to_string();
        match fields {
            None => quote!((#text, ::core::option::Option::None)),
            Some(fields) => {
                let shapes = field_shapes(fields);
                quote!((#text, ::core::option::Option::Some(#shapes)))
            }
        }
    });
    quote! {
        impl #schema::Data for #ident {
            fn decode(
                value: &::donder_language::data::Spanned<::donder_language::data::DataValue>,
                decoder: &mut #schema::Decoder,
            ) -> ::core::option::Option<Self> {
                use ::donder_language::data::DataValue;
                match &value.value {
                    DataValue::Variant(name) => match name.value.as_str() {
                        #(#bare,)*
                        _ => decoder.mismatch(value, #expected),
                    },
                    DataValue::Record(ty, fields) => match ty.value.as_str() {
                        #(#records)*
                        _ => decoder.mismatch(value, #expected),
                    },
                    _ => decoder.mismatch(value, #expected),
                }
            }
            fn encode(&self) -> ::donder_language::data::DataValue {
                match self {
                    #(#encodes,)*
                }
            }
            fn shape(schema: &mut #schema::Schema) -> #schema::Shape {
                schema.named(#text, |schema| #schema::Definition::Variants(vec![#(#shapes),*]))
            }
        }
    }
}
