//! stdin ingestion: a reader thread that extracts one float per line and
//! forwards it to the render loop over an mpsc channel.

use std::io::{self, BufRead};
use std::sync::mpsc::Sender;
use std::thread;

/// Extract the **first** float found anywhere in `line`.
///
/// Lenient by design so the tool can sit downstream of arbitrary output:
/// accepts bare `33.7`, embedded `average rate: 33.746`, signs, leading dots,
/// and `e`/`E` exponents. Returns `None` when the line carries no number.
pub fn parse_first_float(line: &str) -> Option<f64> {
    let bytes = line.as_bytes();
    let mut i = 0;
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
            Ok(v) if v.is_finite() => return Some(v),
            // Parsed but non-finite (overflow, e.g. "1e400"). Skip the whole
            // token — `i` is already at its end — rather than rescanning into
            // it, which would mis-read the exponent digits as a fresh number.
            Ok(_) => continue,
            // Failed to parse (defensive). Resume just past the start.
            Err(_) => i = start + 1,
        }
    }
    None
}

/// Spawn the stdin reader thread.
///
/// Reads lines, forwards each parsed value, and exits silently on EOF or once
/// the receiver is dropped (the render loop has quit).
pub fn spawn_reader(tx: Sender<f64>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let stdin = io::stdin();
        for line in stdin.lock().lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };
            if let Some(v) = parse_first_float(&line) {
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
}
