use crate::openai::create_response::response::{OpenAiCreateResponseResponse, ResponseBody};
use crate::openai::create_response::stream::{
    OpenAiCreateResponseSseData, OpenAiCreateResponseSseEvent, OpenAiCreateResponseSseStreamBody,
    ResponseStreamEvent,
};
use crate::openai::create_response::types as rt;
use crate::transform::utils::TransformError;

fn with_status(mut body: ResponseBody, status: rt::ResponseStatus) -> ResponseBody {
    body.status = Some(status);
    body
}

fn push_event(events: &mut Vec<OpenAiCreateResponseSseEvent>, event: ResponseStreamEvent) {
    events.push(OpenAiCreateResponseSseEvent {
        event: None,
        data: OpenAiCreateResponseSseData::Event(event),
    });
}

fn take_sequence(next_sequence_number: &mut u64) -> u64 {
    let sequence_number = *next_sequence_number;
    *next_sequence_number = next_sequence_number.saturating_add(1);
    sequence_number
}

impl TryFrom<OpenAiCreateResponseResponse> for OpenAiCreateResponseSseStreamBody {
    type Error = TransformError;

    fn try_from(value: OpenAiCreateResponseResponse) -> Result<Self, TransformError> {
        match value {
            OpenAiCreateResponseResponse::Success { body, .. } => {
                let mut events = Vec::new();
                let mut next_sequence_number = 0_u64;

                let in_progress = with_status(body.clone(), rt::ResponseStatus::InProgress);
                let sequence_number = take_sequence(&mut next_sequence_number);
                push_event(
                    &mut events,
                    ResponseStreamEvent::Created {
                        response: in_progress.clone(),
                        sequence_number,
                    },
                );
                let sequence_number = take_sequence(&mut next_sequence_number);
                push_event(
                    &mut events,
                    ResponseStreamEvent::InProgress {
                        response: in_progress,
                        sequence_number,
                    },
                );

                let final_status = body.status.clone().unwrap_or_else(|| {
                    if body.error.is_some() {
                        rt::ResponseStatus::Failed
                    } else if body.incomplete_details.is_some() {
                        rt::ResponseStatus::Incomplete
                    } else {
                        rt::ResponseStatus::Completed
                    }
                });

                match final_status {
                    rt::ResponseStatus::Failed => {
                        let sequence_number = take_sequence(&mut next_sequence_number);
                        push_event(
                            &mut events,
                            ResponseStreamEvent::Failed {
                                response: with_status(body, rt::ResponseStatus::Failed),
                                sequence_number,
                            },
                        )
                    }
                    rt::ResponseStatus::Incomplete => {
                        let sequence_number = take_sequence(&mut next_sequence_number);
                        push_event(
                            &mut events,
                            ResponseStreamEvent::Incomplete {
                                response: with_status(body, rt::ResponseStatus::Incomplete),
                                sequence_number,
                            },
                        )
                    }
                    _ => {
                        let sequence_number = take_sequence(&mut next_sequence_number);
                        push_event(
                            &mut events,
                            ResponseStreamEvent::Completed {
                                response: with_status(body, rt::ResponseStatus::Completed),
                                sequence_number,
                            },
                        )
                    }
                }

                events.push(OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Done("[DONE]".to_string()),
                });

                Ok(OpenAiCreateResponseSseStreamBody { events })
            }
            OpenAiCreateResponseResponse::Error { body, .. } => {
                let mut events = Vec::new();
                let error = body.error;

                events.push(OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Event(ResponseStreamEvent::Error {
                        error: crate::openai::create_response::stream::ResponseStreamErrorPayload {
                            type_: error.type_,
                            code: error.code,
                            message: error.message,
                            param: error.param,
                        },
                        sequence_number: 0,
                    }),
                });
                events.push(OpenAiCreateResponseSseEvent {
                    event: None,
                    data: OpenAiCreateResponseSseData::Done("[DONE]".to_string()),
                });

                Ok(OpenAiCreateResponseSseStreamBody { events })
            }
        }
    }
}
