use super::{
    answer::{Answer, Retrieved},
    openrouter::{Message, Part, Request, Role, Tool, ToolChoice},
    prompt::{SKIP, SUBMIT_ANSWER, TRIAGE_PROMPT, skip_tool, submit_answer_tool, system_prompt},
    transcript::Context,
};

const TRIAGE_TOKENS: u32 = 512;
const MAX_STEPS: usize = 10;
const MAX_DOCS_IMAGES: usize = 4;
const ANSWER_TOKENS: u32 = 4096;

pub async fn needs_reply(env: &crate::env::Env, transcript: &str) -> Result<bool, anyhow::Error> {
    let messages = [
        Message::new(Role::System, TRIAGE_PROMPT),
        Message::new(Role::User, transcript),
    ];

    let reply = super::openrouter::complete(
        env,
        Request {
            model: &env.ai_support_triage_model,
            messages: &messages,
            tools: &[],
            tool_choice: None,
            max_tokens: Some(TRIAGE_TOKENS),
            temperature: Some(0.0),
        },
    )
    .await?;

    let decision = reply.text().to_uppercase();

    tracing::debug!(decision = %decision.trim(), "ai support triage");

    if decision.contains("SKIP") && !decision.contains("REPLY") {
        return Ok(false);
    }

    if !decision.contains("REPLY") {
        tracing::warn!(
            model = %env.ai_support_triage_model,
            "ai support triage answered with neither word, answering anyway"
        );
    }

    Ok(true)
}

pub enum Outcome {
    Answer(Answer),
    Skip,
    HandOver(String),
}

fn opening_message(context: &Context) -> Message {
    if context.images.is_empty() {
        return Message::new(Role::User, context.text.clone());
    }

    let mut parts = vec![Part::Text {
        text: context.text.clone(),
    }];

    for image in &context.images {
        parts.push(Part::Text {
            text: format!("Image posted in the thread: {}", image.filename),
        });
        parts.push(Part::image(image.mime_type, &image.data));
    }

    Message::parts(Role::User, parts)
}

pub async fn generate(env: &crate::env::Env, context: &Context) -> Result<Outcome, anyhow::Error> {
    let mcp = super::mcp_client(env);

    let mcp_tools = mcp.tools().await?;
    let mut tools: Vec<Tool<'_>> = mcp_tools
        .iter()
        .map(|tool| {
            Tool::function(
                &tool.name,
                tool.description.as_deref(),
                tool.input_schema.clone(),
            )
        })
        .collect();
    tools.push(submit_answer_tool());
    tools.push(skip_tool());

    let mut messages = vec![
        Message::new(Role::System, system_prompt(super::docs_url(env))),
        opening_message(context),
    ];
    let mut retrieved = Retrieved::default();
    let mut docs_images = 0;

    for step in 0..MAX_STEPS {
        let last_step = step == MAX_STEPS - 1;

        let reply = super::openrouter::complete(
            env,
            Request {
                model: &env.ai_support_model,
                messages: &messages,
                tools: &tools,
                tool_choice: Some(if last_step {
                    ToolChoice::required(SUBMIT_ANSWER)
                } else {
                    ToolChoice::Mode("auto")
                }),
                max_tokens: Some(ANSWER_TOKENS),
                temperature: Some(0.2),
            },
        )
        .await?;

        let tool_calls = reply.tool_calls.clone().unwrap_or_default();

        if tool_calls.is_empty() {
            messages.push(reply);
            messages.push(Message::new(
                Role::User,
                format!("Call {SUBMIT_ANSWER} with your answer, or {SKIP} if the thread needs no answer from you."),
            ));

            continue;
        }

        messages.push(reply);

        for call in tool_calls {
            match call.function.name.as_str() {
                SUBMIT_ANSWER => {
                    return Ok(Outcome::Answer(super::answer::parse(
                        &call.function.arguments,
                        &retrieved,
                    )?));
                }
                SKIP => return Ok(skipped(&call.function.arguments)),
                name => {
                    let arguments = serde_json::from_str(&call.function.arguments)
                        .unwrap_or(serde_json::Value::Object(Default::default()));

                    let result = match mcp.call(name, arguments).await {
                        Ok(result) => result,
                        Err(err) => {
                            tracing::warn!("ai support tool {name} failed: {err:?}");
                            messages.push(Message::tool_result(
                                call.id,
                                format!("The tool failed: {err}"),
                            ));

                            continue;
                        }
                    };

                    if !result.is_error {
                        retrieved.push(&result.text());
                    }

                    messages.push(Message::tool_result(call.id, result.text()));

                    let mut parts = Vec::new();
                    for (mime_type, data) in result.images() {
                        if docs_images >= MAX_DOCS_IMAGES {
                            break;
                        }

                        docs_images += 1;
                        parts.push(Part::image(mime_type, data));
                    }

                    if !parts.is_empty() {
                        parts.insert(
                            0,
                            Part::Text {
                                text: format!("Images returned by {name}:"),
                            },
                        );
                        messages.push(Message::parts(Role::User, parts));
                    }
                }
            }
        }
    }

    Err(anyhow::anyhow!(
        "ai support did not submit an answer within {MAX_STEPS} steps"
    ))
}

fn skipped(arguments: &str) -> Outcome {
    #[derive(serde::Deserialize, Default)]
    struct Skipped {
        #[serde(default)]
        reason: String,
        #[serde(default)]
        hand_over: bool,
    }

    let skipped: Skipped = serde_json::from_str(arguments).unwrap_or_default();

    tracing::debug!(
        reason = %skipped.reason,
        hand_over = skipped.hand_over,
        "ai support decided not to answer"
    );

    if skipped.hand_over {
        Outcome::HandOver(skipped.reason)
    } else {
        Outcome::Skip
    }
}
