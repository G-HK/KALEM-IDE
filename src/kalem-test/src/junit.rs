use crate::{Outcome, TestCase, TestRun};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Debug, PartialEq, Eq)]
pub enum JunitError {
    Xml(String),
    /// No `<testsuite>` / `<testsuites>` element found.
    NotJunit,
}

impl std::fmt::Display for JunitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JunitError::Xml(e) => write!(f, "invalid XML: {e}"),
            JunitError::NotJunit => write!(f, "not a JUnit report (no <testsuite> element)"),
        }
    }
}

impl std::error::Error for JunitError {}

fn attr(e: &BytesStart<'_>, key: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.as_ref() == key.as_bytes())
        .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
}

/// Which child element of `<testcase>` we are collecting text for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Child {
    Failure,
    Error,
    Skipped,
    Other,
}

/// Parse a JUnit XML report (root `<testsuites>` or a single `<testsuite>`).
pub fn parse_junit(xml: &str) -> Result<TestRun, JunitError> {
    let mut reader = Reader::from_str(xml);
    let mut run = TestRun::default();
    let mut suites: Vec<String> = Vec::new();
    let mut case: Option<TestCase> = None;
    let mut child: Option<Child> = None;
    let mut text = String::new();
    let mut saw_suite = false;

    loop {
        let ev = reader
            .read_event()
            .map_err(|e| JunitError::Xml(e.to_string()))?;
        // Name of an element that closes at this event. A self-closing tag such as
        // `<testcase .../>` opens and closes in one event, so it sets this immediately.
        let mut closes: Option<Vec<u8>> = None;
        match &ev {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name();
                match name.as_ref() {
                    b"testsuite" => {
                        saw_suite = true;
                        suites.push(attr(e, "name").unwrap_or_default());
                    }
                    b"testcase" => {
                        case = Some(TestCase {
                            suite: suites.last().cloned().unwrap_or_default(),
                            class: attr(e, "classname").filter(|c| !c.is_empty()),
                            name: attr(e, "name").unwrap_or_default(),
                            duration_secs: attr(e, "time")
                                .and_then(|t| t.parse().ok())
                                .unwrap_or(0.0),
                            outcome: Outcome::Passed,
                            message: None,
                            details: None,
                            file: attr(e, "file"),
                        });
                    }
                    b"failure" | b"error" | b"skipped" => {
                        let kind = match name.as_ref() {
                            b"failure" => Child::Failure,
                            b"error" => Child::Error,
                            _ => Child::Skipped,
                        };
                        if let Some(c) = case.as_mut() {
                            c.outcome = match kind {
                                Child::Failure => Outcome::Failed,
                                Child::Error => Outcome::Error,
                                _ => Outcome::Skipped,
                            };
                            c.message = attr(e, "message");
                        }
                        child = Some(kind);
                        text.clear();
                    }
                    b"system-out" | b"system-err" => {
                        child = Some(Child::Other);
                        text.clear();
                    }
                    _ => {}
                }
                if matches!(ev, Event::Empty(_)) {
                    closes = Some(name.as_ref().to_vec());
                }
            }
            Event::Text(t) => {
                if child.is_some() {
                    text.push_str(&t.decode().map_err(|e| JunitError::Xml(e.to_string()))?);
                }
            }
            Event::CData(t) => {
                if child.is_some() {
                    text.push_str(&String::from_utf8_lossy(t));
                }
            }
            Event::GeneralRef(r) => {
                if child.is_some() {
                    if let Ok(Some(ch)) = r.resolve_char_ref() {
                        text.push(ch);
                    } else {
                        let name = r.decode().map_err(|e| JunitError::Xml(e.to_string()))?;
                        match name.as_ref() {
                            "lt" => text.push('<'),
                            "gt" => text.push('>'),
                            "amp" => text.push('&'),
                            "quot" => text.push('"'),
                            "apos" => text.push('\''),
                            other => {
                                text.push('&');
                                text.push_str(other);
                                text.push(';');
                            }
                        }
                    }
                }
            }
            Event::End(e) => closes = Some(e.name().as_ref().to_vec()),
            _ => {}
        }

        if let Some(name) = closes {
            match name.as_slice() {
                b"testsuite" => {
                    suites.pop();
                }
                b"failure" | b"error" | b"skipped" => {
                    if let (Some(c), Some(_)) = (case.as_mut(), child) {
                        let t = text.trim();
                        if !t.is_empty() {
                            c.details = Some(t.to_string());
                        }
                    }
                    child = None;
                }
                b"system-out" | b"system-err" => child = None,
                b"testcase" => {
                    if let Some(c) = case.take() {
                        run.cases.push(c);
                    }
                }
                _ => {}
            }
        }
    }

    if !saw_suite {
        return Err(JunitError::NotJunit);
    }
    Ok(run)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<testsuites>
  <testsuite name="math" tests="4" failures="1" errors="1" skipped="1" time="1.5">
    <testcase classname="math.Calc" name="adds" time="0.010"/>
    <testcase classname="math.Calc" name="divides" time="0.500">
      <failure message="expected 2 but was 3 &amp; more" type="AssertionError">trace line 1
trace &lt;line&gt; 2</failure>
    </testcase>
    <testcase classname="math.Calc" name="crashes" time="0.900">
      <error message="NullPointerException"><![CDATA[at Calc.crash(Calc.java:10)]]></error>
    </testcase>
    <testcase classname="math.Calc" name="later" time="0">
      <skipped message="not ready"/>
    </testcase>
  </testsuite>
  <testsuite name="io">
    <testcase classname="io.File" name="reads" time="0.090" file="src/io.rs"/>
  </testsuite>
</testsuites>"#;

    #[test]
    fn parses_all_outcomes_including_self_closing_cases() {
        let run = parse_junit(SAMPLE).unwrap();
        assert_eq!(run.cases.len(), 5);
        let s = run.summary();
        assert_eq!(
            (s.total, s.passed, s.failed, s.errors, s.skipped),
            (5, 2, 1, 1, 1)
        );
        assert!(!s.is_green());
        assert!((s.duration_secs - 1.5).abs() < 1e-9);
    }

    #[test]
    fn keeps_messages_details_and_entities() {
        let run = parse_junit(SAMPLE).unwrap();
        let f = &run.failures();
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].full_name(), "math.Calc.divides");
        assert_eq!(f[0].message.as_deref(), Some("expected 2 but was 3 & more"));
        assert_eq!(
            f[0].details.as_deref(),
            Some("trace line 1\ntrace <line> 2")
        );
        assert_eq!(f[1].details.as_deref(), Some("at Calc.crash(Calc.java:10)"));
        assert_eq!(run.cases[4].file.as_deref(), Some("src/io.rs"));
    }

    #[test]
    fn groups_by_suite_and_finds_slowest() {
        let run = parse_junit(SAMPLE).unwrap();
        let suites = run.by_suite();
        assert_eq!(suites.len(), 2);
        assert_eq!(suites[0].0, "math");
        assert_eq!(suites[0].1.total, 4);
        assert_eq!(suites[1].1.passed, 1);
        assert_eq!(run.slowest(1)[0].name, "crashes");
    }

    #[test]
    fn single_testsuite_root_and_errors() {
        let one = r#"<testsuite name="s"><testcase name="t" time="0.1"/></testsuite>"#;
        let run = parse_junit(one).unwrap();
        assert_eq!(run.summary().passed, 1);
        assert!(run.summary().is_green());

        assert_eq!(parse_junit("<html></html>"), Err(JunitError::NotJunit));
        assert!(matches!(
            parse_junit("<testsuite><oops></testsuite>"),
            Err(JunitError::Xml(_))
        ));
    }

    #[test]
    fn overview_text_mentions_failures() {
        let text = parse_junit(SAMPLE).unwrap().overview(2);
        assert!(text.starts_with("FAIL"));
        assert!(text.contains("math.Calc.divides: expected 2 but was 3 & more"));
        assert!(text.contains("Slowest:"));
    }
}
