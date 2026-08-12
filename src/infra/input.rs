//! stdin ingestion: a reader thread that extracts one float per line and
//! forwards it to the render loop over an mpsc channel. The extraction
//! strategy is a [`Parser`] chosen with `--parser`.

use std::io::{self, BufRead};
use std::sync::mpsc::Sender;
use std::thread;

/// Extract the **first** float found anywhere in `line`.
///
/// Lenient by design so the tool can sit downstream of arbitrary output:
/// accepts bare `33.7`, embedded `average rate: 33.746`, signs, leading dots,
/// and `e`/`E` exponents. Returns `None` when the line carries no number.
pub fn parse_first_float(line: &str) -> Option<f64> {
    scan_float(line, 0).map(|(v, _, _)| v)
}

/// Find the next finite float in `line` at or after byte index `from`.
/// Returns `(value, start, end)` (the token's byte range), so callers can
/// resume scanning at `end` or require the token to start at a known position.
fn scan_float(line: &str, from: usize) -> Option<(f64, usize, usize)> {
    let bytes = line.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        let c = bytes[i] as char;
        // A float token starts with a digit, or a sign/dot immediately
        // followed by a digit (so we don't latch onto a stray '-' or '.').
        let starts = c.is_ascii_digit()
            || ((c == '-' || c == '+' || c == '.')
                && i + 1 < bytes.len()
                && (bytes[i + 1] as char).is_ascii_digit());

        if !starts {
            i += 1;
            continue;
        }

        let start = i;
        let mut seen_dot = false;
        let mut seen_exp = false;
        if c == '-' || c == '+' {
            i += 1;
        }
        while i < bytes.len() {
            let d = bytes[i] as char;
            if d.is_ascii_digit() {
                i += 1;
            } else if d == '.' && !seen_dot && !seen_exp {
                seen_dot = true;
                i += 1;
            } else if (d == 'e' || d == 'E') && !seen_exp {
                // Only consume the exponent if a digit actually follows (after
                // an optional sign). Otherwise stop here so a dangling `e`/`e+`
                // doesn't swallow the valid mantissa (e.g. "1e+" -> 1).
                let mut j = i + 1;
                if j < bytes.len() && (bytes[j] == b'-' || bytes[j] == b'+') {
                    j += 1;
                }
                if j < bytes.len() && (bytes[j] as char).is_ascii_digit() {
                    seen_exp = true;
                    i = j; // exponent digits consumed by the loop
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        match line[start..i].parse::<f64>() {
            Ok(v) if v.is_finite() => return Some((v, start, i)),
            // Parsed but non-finite (overflow, e.g. "1e400"). Skip the whole
            // token (`i` is already at its end) rather than rescanning into
            // it, which would mis-read the exponent digits as a fresh number.
            Ok(_) => continue,
            // Failed to parse (defensive). Resume just past the start.
            Err(_) => i = start + 1,
        }
    }
    None
}

/// How to extract one value from a line of input. Chosen with `--parser`.
#[derive(Clone, Debug, PartialEq)]
pub enum Parser {
    /// First number on the line (the default, and the historical behavior).
    First,
    /// Last number on the line.
    Last,
    /// N-th number on the line, 1-based.
    Nth(usize),
    /// Number following `NAME=` or `NAME:` (optionally spaced).
    Key(String),
    /// Round-trip time from `ping` output: the number in `time=12.3 ms`,
    /// including the Windows forms `time=12ms` and `time<1ms`.
    Ping,
}

/// Parser names shown in help and error messages.
pub const PARSER_SPECS: &str = "first, last, nth:N, key:NAME, ping";

impl Parser {
    /// Parse a `--parser` spec. Returns a usable error message on failure.
    pub fn from_spec(spec: &str) -> Result<Parser, String> {
        match spec {
            "first" => Ok(Parser::First),
            "last" => Ok(Parser::Last),
            "ping" => Ok(Parser::Ping),
            _ => {
                if let Some(n) = spec.strip_prefix("nth:") {
                    return match n.parse::<usize>() {
                        Ok(n) if n >= 1 => Ok(Parser::Nth(n)),
                        _ => Err(format!("nth needs a 1-based index, got '{n}'")),
                    };
                }
                if let Some(k) = spec.strip_prefix("key:") {
                    if k.is_empty() {
                        return Err("key needs a name, e.g. key:rate".into());
                    }
                    return Ok(Parser::Key(k.to_string()));
                }
                Err(format!("unknown parser '{spec}' (available: {PARSER_SPECS})"))
            }
        }
    }

    /// Extract this parser's value from `line`, or `None` to skip the line.
    pub fn parse(&self, line: &str) -> Option<f64> {
        match self {
            Parser::First => parse_first_float(line),
            Parser::Last => {
                let mut last = None;
                let mut from = 0;
                while let Some((v, _, end)) = scan_float(line, from) {
                    last = Some(v);
                    from = end;
                }
                last
            }
            Parser::Nth(n) => {
                let mut from = 0;
                let mut count = 0;
                while let Some((v, _, end)) = scan_float(line, from) {
                    count += 1;
                    if count == *n {
                        return Some(v);
                    }
                    from = end;
                }
                None
            }
            Parser::Key(key) => parse_after_key(line, key),
            Parser::Ping => parse_ping(line),
        }
    }
}

/// Value of the first `KEY = <num>` / `KEY: <num>` occurrence in `line`.
/// The key must not be preceded by an alphanumeric (so `key:time` does not
/// match inside `uptime`), and the number must directly follow the separator.
fn parse_after_key(line: &str, key: &str) -> Option<f64> {
    let bytes = line.as_bytes();
    let mut search = 0;
    while let Some(off) = line[search..].find(key) {
        let pos = search + off;
        search = pos + 1;
        if pos > 0 && bytes[pos - 1].is_ascii_alphanumeric() {
            continue; // inside a longer word
        }
        let mut i = pos + key.len();
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if i >= bytes.len() || (bytes[i] != b'=' && bytes[i] != b':') {
            continue;
        }
        i += 1;
        while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
            i += 1;
        }
        if let Some((v, start, _)) = scan_float(line, i) {
            if start == i {
                return Some(v);
            }
        }
    }
    None
}

/// RTT from a `ping` reply line: the number right after `time=` (or `time<`,
/// which Windows prints for sub-millisecond replies). Summary lines like
/// `... 0% packet loss, time 3005ms` don't match: no separator after `time`.
fn parse_ping(line: &str) -> Option<f64> {
    let bytes = line.as_bytes();
    let mut search = 0;
    while let Some(off) = line[search..].find("time") {
        let pos = search + off;
        search = pos + 1;
        if pos > 0 && bytes[pos - 1].is_ascii_alphanumeric() {
            continue;
        }
        let i = pos + "time".len();
        if i >= bytes.len() || (bytes[i] != b'=' && bytes[i] != b'<') {
            continue;
        }
        if let Some((v, start, _)) = scan_float(line, i + 1) {
            if start == i + 1 {
                return Some(v);
            }
        }
    }
    None
}

/// Spawn the stdin reader thread.
///
/// Reads lines, forwards each value extracted by `parser`, and exits silently
/// on EOF or once the receiver is dropped (the render loop has quit).
pub fn spawn_reader(tx: Sender<f64>, parser: Parser) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Some(v) = parser.parse(&line) {
                if tx.send(v).is_err() {
                    break; // receiver gone
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_float() {
        assert_eq!(parse_first_float("33.7"), Some(33.7));
    }

    #[test]
    fn embedded_in_label() {
        assert_eq!(parse_first_float("average rate: 33.746"), Some(33.746));
    }

    #[test]
    fn first_number_wins() {
        assert_eq!(parse_first_float("min: 0.014s max: 0.102s"), Some(0.014));
    }

    #[test]
    fn signed_exponent() {
        assert_eq!(parse_first_float("v=-1.2e3 done"), Some(-1200.0));
    }

    #[test]
    fn leading_dot() {
        assert_eq!(parse_first_float("value .5 here"), Some(0.5));
    }

    #[test]
    fn no_number() {
        assert_eq!(parse_first_float("no number here"), None);
    }

    #[test]
    fn lone_punctuation_is_skipped() {
        // A bare '.' and '-' carry no digit; the real number is found after.
        assert_eq!(parse_first_float(". - then 7"), Some(7.0));
    }

    #[test]
    fn integer_is_a_float() {
        assert_eq!(parse_first_float("count 42"), Some(42.0));
    }

    #[test]
    fn overflow_token_is_skipped_not_misread() {
        // Must NOT return the exponent digits (400 / 308) as a value.
        assert_eq!(parse_first_float("1e400"), None);
        assert_eq!(parse_first_float("5e308"), None);
    }

    #[test]
    fn overflow_then_real_number_recovers() {
        // After skipping the overflow token, the next valid number is found.
        assert_eq!(parse_first_float("rate 1e400 then 7"), Some(7.0));
    }

    #[test]
    fn dangling_exponent_keeps_mantissa() {
        assert_eq!(parse_first_float("1e+"), Some(1.0));
        assert_eq!(parse_first_float("12e+x"), Some(12.0));
        assert_eq!(parse_first_float("3.5e- done"), Some(3.5));
    }

    #[test]
    fn valid_exponent_still_parses() {
        assert_eq!(parse_first_float("x 1.5e2 y"), Some(150.0));
    }

    const PING_LINE: &str = "64 bytes from 8.8.8.8: icmp_seq=1 ttl=117 time=12.3 ms";

    #[test]
    fn ping_extracts_rtt_not_byte_count() {
        assert_eq!(Parser::Ping.parse(PING_LINE), Some(12.3));
    }

    #[test]
    fn ping_windows_forms() {
        assert_eq!(Parser::Ping.parse("Reply from 1.1.1.1: bytes=32 time=41ms TTL=55"), Some(41.0));
        assert_eq!(Parser::Ping.parse("Reply from 1.1.1.1: bytes=32 time<1ms TTL=55"), Some(1.0));
    }

    #[test]
    fn ping_skips_summary_lines() {
        // "time 3005ms" has no separator; "round-trip" stats carry no `time=`.
        assert_eq!(
            Parser::Ping.parse("4 packets transmitted, 4 received, 0% packet loss, time 3005ms"),
            None
        );
        assert_eq!(
            Parser::Ping.parse("rtt min/avg/max/mdev = 13.318/14.502/16.119/1.104 ms"),
            None
        );
    }

    #[test]
    fn last_and_nth() {
        assert_eq!(Parser::Last.parse(PING_LINE), Some(12.3));
        assert_eq!(Parser::Nth(2).parse("min: 0.014s max: 0.102s"), Some(0.102));
        assert_eq!(Parser::Nth(3).parse("min: 0.014s max: 0.102s"), None);
    }

    #[test]
    fn key_matches_equals_and_colon_with_spacing() {
        assert_eq!(Parser::Key("rate".into()).parse("rate: 33.7"), Some(33.7));
        assert_eq!(Parser::Key("rate".into()).parse("x=1 rate = 33.7"), Some(33.7));
        assert_eq!(Parser::Key("ttl".into()).parse(PING_LINE), Some(117.0));
    }

    #[test]
    fn key_requires_word_boundary_and_adjacent_number() {
        // `time` must not match inside `uptime`.
        assert_eq!(Parser::Key("time".into()).parse("uptime=99 time=3"), Some(3.0));
        // A key with no number directly after its separator doesn't grab a
        // number from later in the line.
        assert_eq!(Parser::Key("rate".into()).parse("rate: n/a next=5"), None);
    }

    #[test]
    fn spec_round_trip() {
        assert_eq!(Parser::from_spec("first"), Ok(Parser::First));
        assert_eq!(Parser::from_spec("last"), Ok(Parser::Last));
        assert_eq!(Parser::from_spec("ping"), Ok(Parser::Ping));
        assert_eq!(Parser::from_spec("nth:3"), Ok(Parser::Nth(3)));
        assert_eq!(Parser::from_spec("key:rate"), Ok(Parser::Key("rate".into())));
        assert!(Parser::from_spec("nth:0").is_err());
        assert!(Parser::from_spec("key:").is_err());
        assert!(Parser::from_spec("bogus").is_err());
    }
}
