//! `#[iram]` links a function, or every function of an impl, into ESP32
//! instruction RAM when the annotated crate's `iram` feature is on.

use proc_macro::{Delimiter, Group, TokenStream, TokenTree};

#[proc_macro_attribute]
pub fn iram(_: TokenStream, item: TokenStream) -> TokenStream {
    let mut tokens: Vec<TokenTree> = item.into_iter().collect();
    if declares_fn(&tokens) {
        return placement().into_iter().chain(tokens).collect();
    }
    let Some(TokenTree::Group(body)) = tokens.last_mut() else {
        panic!("#[iram] applies to a fn or an impl");
    };
    let mut placed = Group::new(Delimiter::Brace, place_each(body.stream()));
    placed.set_span(body.span());
    *body = placed;
    tokens.into_iter().collect()
}

fn placement() -> TokenStream {
    r#"#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]"#
        .parse()
        .expect("the placement attribute is valid Rust")
}

/// Whether these top-level tokens declare a function: `fn` and its name.
fn declares_fn(tokens: &[TokenTree]) -> bool {
    tokens.windows(2).any(|pair| {
        matches!(pair, [TokenTree::Ident(keyword), TokenTree::Ident(_)] if keyword.to_string() == "fn")
    })
}

/// An impl body with the placement before each function. An item ends at a
/// top-level `;` or brace-delimited body.
fn place_each(body: TokenStream) -> TokenStream {
    let mut placed = TokenStream::new();
    let mut item = Vec::new();
    for token in body {
        let ends = match &token {
            TokenTree::Punct(punct) => punct.as_char() == ';',
            TokenTree::Group(group) => group.delimiter() == Delimiter::Brace,
            _ => false,
        };
        item.push(token);
        if ends {
            if declares_fn(&item) {
                placed.extend(placement());
            }
            placed.extend(item.drain(..));
        }
    }
    placed.extend(item);
    placed
}
