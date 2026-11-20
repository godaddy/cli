//! Reads entries: directives and records.

use crate::error::{ErrorKind, ParseError};
use crate::lexer::{self, Line};
use crate::name::Name;
use crate::options::ParseOptions;
use crate::rdata_parse;
use crate::ttl::parse_ttl;
use crate::zone::{Record, Zone};

/// Classes this crate recognises. Only `IN` is supported.
const CLASSES: &[&str] = &["IN", "CH", "HS", "CS"];

/// Read the zone file in `input`. See the crate documentation.
pub(crate) fn parse(input: &str, options: &ParseOptions) -> Result<Zone, ParseError> {
    options.validate()?;
    if input.len() > options.max_input_bytes {
        return Err(ParseError::new(
            ErrorKind::Limit,
            0,
            0,
            format!(
                "input is {} bytes; the limit is {}",
                input.len(),
                options.max_input_bytes
            ),
        ));
    }
    let mut state = State {
        origin: options.origin.clone(),
        zone_origin: options.origin.clone(),
        dollar_ttl: None,
        last_ttl: None,
        last_owner: None,
    };
    let mut records = Vec::new();
    for line in lexer::lex(input)? {
        let Some(first) = line.tokens.first() else {
            continue;
        };
        if !line.indented
            && let Some(word) = first.word()
            && word.starts_with('$')
        {
            state.directive(&line, word)?;
            continue;
        }
        if records.len() >= options.max_records {
            return Err(ParseError::new(
                ErrorKind::Limit,
                line.number,
                0,
                format!("more than {} records", options.max_records),
            ));
        }
        records.push(state.record(&line, options)?);
    }
    Ok(Zone {
        origin: state.zone_origin,
        records,
    })
}

/// What earlier entries set for later ones.
struct State {
    /// The current origin, which `$ORIGIN` changes.
    origin: Option<Name>,
    /// The origin the zone is reported with.
    zone_origin: Option<Name>,
    /// The value of the last `$TTL`.
    dollar_ttl: Option<u32>,
    /// The TTL of the last record that gave one.
    last_ttl: Option<u32>,
    last_owner: Option<Name>,
}

impl State {
    fn directive(&mut self, line: &Line, word: &str) -> Result<(), ParseError> {
        let Some(first) = line.tokens.first() else {
            return Ok(());
        };
        match word.to_ascii_uppercase().as_str() {
            "$ORIGIN" => {
                let [_, arg] = line.tokens.as_slice() else {
                    return Err(first.error(ErrorKind::Syntax, "`$ORIGIN` takes one name"));
                };
                let text = arg.word().ok_or_else(|| {
                    arg.error(
                        ErrorKind::Syntax,
                        "`$ORIGIN` takes a name, not a quoted string",
                    )
                })?;
                let name = Name::parse_at(text, self.origin.as_ref(), arg.line, arg.column)?;
                if self.zone_origin.is_none() {
                    self.zone_origin = Some(name.clone());
                }
                self.origin = Some(name);
            }
            "$TTL" => {
                let [_, arg] = line.tokens.as_slice() else {
                    return Err(first.error(ErrorKind::Syntax, "`$TTL` takes one time"));
                };
                let text = arg.word().ok_or_else(|| {
                    arg.error(
                        ErrorKind::Syntax,
                        "`$TTL` takes a time, not a quoted string",
                    )
                })?;
                self.dollar_ttl =
                    Some(parse_ttl(text).map_err(|m| arg.error(ErrorKind::InvalidValue, m))?);
            }
            "$INCLUDE" | "$GENERATE" => {
                return Err(first.error(
                    ErrorKind::Unsupported,
                    format!("`{word}` is not supported; write the records out in full"),
                ));
            }
            _ => {
                return Err(first.error(ErrorKind::Syntax, format!("unknown directive `{word}`")));
            }
        }
        Ok(())
    }

    fn record(&mut self, line: &Line, options: &ParseOptions) -> Result<Record, ParseError> {
        let Some(first) = line.tokens.first() else {
            return Err(ParseError::new(
                ErrorKind::Syntax,
                line.number,
                0,
                "empty entry",
            ));
        };
        let mut index = 0;
        let owner = if line.indented {
            self.last_owner.clone().ok_or_else(|| {
                first.error(
                    ErrorKind::Syntax,
                    "the first record needs a name; an indented line reuses the previous one",
                )
            })?
        } else {
            let text = first.word().ok_or_else(|| {
                first.error(ErrorKind::Syntax, "a record name cannot be a quoted string")
            })?;
            index = 1;
            Name::parse_at(text, self.origin.as_ref(), first.line, first.column)?
        };

        let mut ttl = None;
        let mut class_seen = false;
        let (type_token, type_word) = loop {
            let Some(token) = line.tokens.get(index) else {
                let at = line.tokens.last().unwrap_or(first);
                return Err(at.error(ErrorKind::Syntax, "record has no type"));
            };
            index += 1;
            let Some(word) = token.word() else {
                return Err(token.error(
                    ErrorKind::Syntax,
                    "expected a record type, found a quoted string",
                ));
            };
            if word.as_bytes().first().is_some_and(u8::is_ascii_digit) {
                if ttl.is_some() {
                    return Err(token.error(ErrorKind::Syntax, "the TTL is given twice"));
                }
                ttl = Some(parse_ttl(word).map_err(|m| token.error(ErrorKind::InvalidValue, m))?);
            } else if CLASSES.iter().any(|c| c.eq_ignore_ascii_case(word)) {
                if class_seen {
                    return Err(token.error(ErrorKind::Syntax, "the class is given twice"));
                }
                if !word.eq_ignore_ascii_case("IN") {
                    return Err(token.error(
                        ErrorKind::Unsupported,
                        format!("class `{word}` is not supported; only `IN` is"),
                    ));
                }
                class_seen = true;
            } else {
                break (token, word);
            }
        };

        let rdata_tokens = line.tokens.get(index..).unwrap_or(&[]);
        let data = rdata_parse::parse(
            type_token,
            type_word,
            rdata_tokens,
            self.origin.as_ref(),
            options,
        )?;

        let ttl = match ttl {
            Some(explicit) => {
                self.last_ttl = Some(explicit);
                explicit
            }
            None => self
                .dollar_ttl
                .or(self.last_ttl)
                .or(options.default_ttl)
                .ok_or_else(|| {
                    type_token.error(
                        ErrorKind::InvalidValue,
                        "record has no TTL; add a `$TTL` line or give one on the record",
                    )
                })?,
        };
        self.last_owner = Some(owner.clone());
        Ok(Record {
            name: owner,
            ttl,
            data,
            line: line.number,
        })
    }
}

#[cfg(test)]
#[path = "parser_tests.rs"]
mod tests;
