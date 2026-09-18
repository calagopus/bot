use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const API_URL: &str = "https://openrouter.ai/api/alpha/decisions";

#[derive(Serialize, Debug, Clone)]
pub struct Criteria<'a> {
    #[serde(rename = "true")]
    pub yes: &'a str,
    #[serde(rename = "false")]
    pub no: &'a str,
}

#[derive(Serialize, Debug, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question<'a> {
    Noul {
        instructions: &'a str,
        criteria: Criteria<'a>,
    },
}

#[derive(Serialize, Debug)]
struct Request<'a> {
    model: &'a str,
    state: &'a str,
    questions: HashMap<&'a str, Question<'a>>,
}

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Answer {
    Noul { noul: f64 },
}

#[derive(Deserialize, Debug, Default)]
struct Usage {
    #[serde(default)]
    cost: f64,
}

#[derive(Deserialize, Debug)]
struct Response {
    #[serde(default)]
    answers: HashMap<String, Answer>,
    #[serde(default)]
    usage: Usage,
    #[serde(default)]
    error: Option<serde_json::Value>,
}

pub struct Decision {
    pub score: f64,
    pub cost: f64,
}

fn decision(body: &str, question: &str) -> Result<Decision, anyhow::Error> {
    let response: Response = serde_json::from_str(body)?;

    if let Some(error) = response.error {
        return Err(anyhow::anyhow!("openrouter returned an error: {error}"));
    }

    let Some(answer) = response.answers.get(question) else {
        return Err(anyhow::anyhow!(
            "openrouter did not answer the question {question}"
        ));
    };

    let Answer::Noul { noul } = answer;

    Ok(Decision {
        score: *noul,
        cost: response.usage.cost,
    })
}

pub async fn ask(
    env: &crate::env::Env,
    model: &str,
    state: &str,
    question: &str,
    asked: Question<'_>,
) -> Result<Decision, anyhow::Error> {
    let body = super::post(
        env,
        API_URL,
        &Request {
            model,
            state,
            questions: HashMap::from([(question, asked)]),
        },
    )
    .await?;

    decision(&body, question)
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUESTION: &str = "needs_reply";

    #[track_caller]
    fn scored(body: &str) -> Decision {
        decision(body, QUESTION).expect("a well formed answer did not score")
    }

    #[track_caller]
    fn rejected(body: &str) {
        if let Ok(decision) = decision(body, QUESTION) {
            panic!(
                "scored {} from a shape it should reject: {body}",
                decision.score
            );
        }
    }

    // decision
    #[test]
    fn reads_a_captured_answer() {
        let decision = scored(
            r#"{"model":"typesafe/jev-1.13-20260917","answers":{"needs_reply":{"type":"noul","noul":0.95}},"usage":{"input_tokens":494,"output_tokens":80,"cost":0.000020748},"id":"gen-dec-1789715484-XfnA6HyB6dNVkgwyecCd","provider":"TypeSafe"}"#,
        );

        assert_eq!(decision.score, 0.95);
        assert_eq!(decision.cost, 0.000020748);
    }

    #[test]
    fn reports_an_error_body() {
        rejected(r#"{"error":{"message":"Model typesafe/nope-9 does not exist","code":400}}"#);
    }

    /// An error can arrive in a 200 body, and nothing says what rode along with it. The error wins
    /// over anything shaped like an answer.
    #[test]
    fn an_error_beats_an_answer_in_the_same_body() {
        rejected(
            r#"{"answers":{"needs_reply":{"type":"noul","noul":0.95}},"error":{"message":"upstream died"}}"#,
        );
    }

    #[test]
    fn rejects_a_body_that_never_answered_the_question() {
        rejected(
            r#"{"answers":{"other_question":{"type":"noul","noul":0.95}},"usage":{"cost":0.1}}"#,
        );
        rejected(r#"{"usage":{"cost":0.1}}"#);
        rejected("{}");
    }

    /// The caller reads an `Err` as "answer anyway" and any score under its threshold as silence,
    /// so an unreadable answer has to fail rather than default to a score of zero.
    #[test]
    fn rejects_answers_it_cannot_read() {
        // A kind of answer this client does not know.
        rejected(r#"{"answers":{"needs_reply":{"type":"boolean","boolean":true}}}"#);

        // The known kind, without its number, or with something that is not one.
        rejected(r#"{"answers":{"needs_reply":{"type":"noul"}}}"#);
        rejected(r#"{"answers":{"needs_reply":{"type":"noul","noul":"0.95"}}}"#);
        rejected(r#"{"answers":{"needs_reply":{"type":"noul","noul":null}}}"#);

        // An answer that is not an object at all.
        rejected(r#"{"answers":{"needs_reply":0.95}}"#);
        rejected(r#"{"answers":[{"type":"noul","noul":0.95}]}"#);
    }

    #[test]
    fn rejects_a_body_that_is_not_json() {
        rejected("");
        rejected("not json");
        rejected(r#"{"answers":{"needs_reply":{"type":"noul","noul":0.95}}"#);
    }
}
