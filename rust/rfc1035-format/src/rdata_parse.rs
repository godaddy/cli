//! Reads the fields after a record's type into [`RData`].

use std::str::FromStr;

use crate::error::{ErrorKind, ParseError};
use crate::lexer::Token;
use crate::name::Name;
use crate::options::ParseOptions;
use crate::rdata::{CustomShape, CustomValue, RData, Soa, Svcb};
use crate::ttl::parse_ttl;

/// A cursor over the tokens of one record's data.
struct Fields<'a> {
    tokens: &'a [Token],
    pos: usize,
    /// The type token, which errors about missing fields point at.
    anchor: &'a Token,
}

impl<'a> Fields<'a> {
    fn next(&mut self, what: &str) -> Result<&'a Token, ParseError> {
        let token = self.tokens.get(self.pos).ok_or_else(|| {
            self.anchor
                .error(ErrorKind::Syntax, format!("missing {what}"))
        })?;
        self.pos += 1;
        Ok(token)
    }

    fn word(&mut self, what: &str) -> Result<(&'a Token, &'a str), ParseError> {
        let token = self.next(what)?;
        let word = token.word().ok_or_else(|| {
            token.error(
                ErrorKind::Syntax,
                format!("{what} cannot be a quoted string"),
            )
        })?;
        Ok((token, word))
    }

    fn uint<T: TryFrom<u64>>(&mut self, what: &str, max: u64) -> Result<T, ParseError> {
        let (token, word) = self.word(what)?;
        let fail = || {
            token.error(
                ErrorKind::InvalidValue,
                format!("{what} `{word}` must be a whole number from 0 to {max}"),
            )
        };
        if word.is_empty() || !word.bytes().all(|b| b.is_ascii_digit()) {
            return Err(fail());
        }
        let value = word.parse::<u64>().map_err(|_| fail())?;
        T::try_from(value).map_err(|_| fail())
    }

    fn time(&mut self, what: &str) -> Result<u32, ParseError> {
        let (token, word) = self.word(what)?;
        parse_ttl(word).map_err(|m| token.error(ErrorKind::InvalidValue, format!("{what}: {m}")))
    }

    fn name(&mut self, what: &str, origin: Option<&Name>) -> Result<Name, ParseError> {
        let (token, word) = self.word(what)?;
        Name::parse_at(word, origin, token.line, token.column)
    }

    fn address<T: FromStr>(&mut self, what: &str) -> Result<T, ParseError> {
        let (token, word) = self.word(what)?;
        word.parse()
            .map_err(|_| token.error(ErrorKind::InvalidValue, format!("`{word}` is not {what}")))
    }

    /// Take every remaining token.
    fn rest(&mut self) -> &'a [Token] {
        let rest = self.tokens.get(self.pos..).unwrap_or(&[]);
        self.pos = self.tokens.len();
        rest
    }

    fn finish(&self) -> Result<(), ParseError> {
        match self.tokens.get(self.pos) {
            None => Ok(()),
            Some(extra) => Err(extra.error(
                ErrorKind::Syntax,
                format!("unexpected extra data `{}`", extra.display()),
            )),
        }
    }
}

/// Read the data of a record of type `type_word`.
pub(crate) fn parse(
    type_token: &Token,
    type_word: &str,
    tokens: &[Token],
    origin: Option<&Name>,
    options: &ParseOptions,
) -> Result<RData, ParseError> {
    if let Some(first) = tokens.first()
        && first.word() == Some("\\#")
    {
        return Err(first.error(
            ErrorKind::Unsupported,
            "generic `\\#` record data (RFC 3597) is not supported",
        ));
    }
    let upper = type_word.to_ascii_uppercase();
    let mut f = Fields {
        tokens,
        pos: 0,
        anchor: type_token,
    };
    let data = match upper.as_str() {
        "A" => RData::A(f.address("an IPv4 address")?),
        "AAAA" => RData::Aaaa(f.address("an IPv6 address")?),
        "NS" => RData::Ns(f.name("name server", origin)?),
        "CNAME" => RData::Cname(f.name("target name", origin)?),
        "MX" => RData::Mx {
            preference: f.uint("preference", u64::from(u16::MAX))?,
            exchange: f.name("mail server", origin)?,
        },
        "TXT" => RData::Txt(txt(&mut f, type_token)?),
        "SRV" => RData::Srv {
            priority: f.uint("priority", u64::from(u16::MAX))?,
            weight: f.uint("weight", u64::from(u16::MAX))?,
            port: f.uint("port", u64::from(u16::MAX))?,
            target: f.name("target", origin)?,
        },
        "CAA" => caa(&mut f)?,
        "TLSA" => tlsa(&mut f, type_token)?,
        "HTTPS" => RData::Https(svcb(&mut f, origin)?),
        "SVCB" => RData::Svcb(svcb(&mut f, origin)?),
        "SOA" => RData::Soa(Soa {
            mname: f.name("primary name server", origin)?,
            rname: f.name("mailbox", origin)?,
            serial: f.uint("serial", u64::from(u32::MAX))?,
            refresh: f.time("refresh")?,
            retry: f.time("retry")?,
            expire: f.time("expire")?,
            minimum: f.time("minimum")?,
        }),
        other => custom(&mut f, type_token, other, origin, options)?,
    };
    f.finish()?;
    Ok(data)
}

fn txt(f: &mut Fields<'_>, type_token: &Token) -> Result<Vec<Vec<u8>>, ParseError> {
    let tokens = f.rest();
    if tokens.is_empty() {
        return Err(type_token.error(ErrorKind::Syntax, "missing text"));
    }
    let mut strings = Vec::new();
    for token in tokens {
        let bytes = token.bytes()?;
        if bytes.is_empty() {
            strings.push(bytes);
        } else {
            // A character-string holds at most 255 bytes, so a longer value is
            // stored as consecutive strings. Readers concatenate them.
            strings.extend(bytes.chunks(255).map(<[u8]>::to_vec));
        }
    }
    Ok(strings)
}

fn caa(f: &mut Fields<'_>) -> Result<RData, ParseError> {
    let flags = f.uint("flags", u64::from(u8::MAX))?;
    let (tag_token, tag) = f.word("tag")?;
    if tag.is_empty() || !tag.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(tag_token.error(
            ErrorKind::InvalidValue,
            format!("tag `{tag}` must be letters and digits, such as `issue`"),
        ));
    }
    let value = f.next("value")?.bytes()?;
    Ok(RData::Caa {
        flags,
        tag: tag.to_owned(),
        value,
    })
}

fn tlsa(f: &mut Fields<'_>, type_token: &Token) -> Result<RData, ParseError> {
    let usage = f.uint("certificate usage", u64::from(u8::MAX))?;
    let selector = f.uint("selector", u64::from(u8::MAX))?;
    let matching_type = f.uint("matching type", u64::from(u8::MAX))?;
    let tokens = f.rest();
    if tokens.is_empty() {
        return Err(type_token.error(ErrorKind::Syntax, "missing certificate data"));
    }
    // RFC 6698 lets the hex be split by whitespace.
    let mut hex = String::new();
    for token in tokens {
        let word = token.word().ok_or_else(|| {
            token.error(
                ErrorKind::Syntax,
                "certificate data cannot be a quoted string",
            )
        })?;
        hex.push_str(word);
    }
    let anchor = tokens.first().unwrap_or(type_token);
    let data = decode_hex(&hex).map_err(|m| anchor.error(ErrorKind::InvalidValue, m))?;
    Ok(RData::Tlsa {
        usage,
        selector,
        matching_type,
        data,
    })
}

fn decode_hex(hex: &str) -> Result<Vec<u8>, String> {
    if !hex.len().is_multiple_of(2) {
        return Err(format!(
            "certificate data has an odd number of hex digits ({})",
            hex.len()
        ));
    }
    hex.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |b: u8| (b as char).to_digit(16);
            match (digit(pair[0]), digit(pair[1])) {
                (Some(hi), Some(lo)) => {
                    u8::try_from(hi * 16 + lo).map_err(|_| "certificate data is not hex".to_owned())
                }
                _ => Err("certificate data must be hex digits only".to_owned()),
            }
        })
        .collect()
}

fn svcb(f: &mut Fields<'_>, origin: Option<&Name>) -> Result<Svcb, ParseError> {
    let priority = f.uint("priority", u64::from(u16::MAX))?;
    let target = f.name("target", origin)?;
    let params = join_tokens(f.rest());
    Ok(Svcb {
        priority,
        target,
        params,
    })
}

/// The tokens as one line of text, restoring the quotes and keeping together
/// tokens that had no space between them (`alpn="h2,h3"`).
fn join_tokens(tokens: &[Token]) -> String {
    let mut out = String::new();
    for (i, token) in tokens.iter().enumerate() {
        if i > 0 && !token.joined {
            out.push(' ');
        }
        out.push_str(&token.display());
    }
    out
}

fn custom(
    f: &mut Fields<'_>,
    type_token: &Token,
    upper: &str,
    origin: Option<&Name>,
    options: &ParseOptions,
) -> Result<RData, ParseError> {
    let Some(shape) = options.custom_types.get(upper) else {
        let generic = upper
            .strip_prefix("TYPE")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
        return Err(if generic {
            type_token.error(
                ErrorKind::Unsupported,
                format!("generic record type `{upper}` (RFC 3597) is not supported"),
            )
        } else {
            type_token.error(
                ErrorKind::UnknownType,
                format!(
                    "unknown record type `{upper}`; accept it with ParseOptions::with_custom_type"
                ),
            )
        });
    };
    let value = match shape {
        CustomShape::Name => CustomValue::Name(f.name("name", origin)?),
        CustomShape::Words => {
            let tokens = f.rest();
            if tokens.is_empty() {
                return Err(type_token.error(ErrorKind::Syntax, "missing data"));
            }
            // A quoted string that follows a word with no space between them
            // (`key="a b"`) belongs to that word.
            let mut words: Vec<String> = Vec::new();
            for token in tokens {
                match words.last_mut() {
                    Some(last) if token.joined => last.push_str(&token.display()),
                    _ => words.push(token.display()),
                }
            }
            CustomValue::Words(words)
        }
    };
    Ok(RData::Custom {
        type_name: upper.to_owned(),
        value,
    })
}
