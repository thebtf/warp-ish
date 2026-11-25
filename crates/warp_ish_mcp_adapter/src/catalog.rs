use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    WhereAmI,
    ListSurfaces,
    CreateTab,
    SplitPane,
    StartPeer,
    SendKey,
    ClosePane,
    CloseTab,
}

const WINDOW: Argument =
    Argument::required("window_id", "Window ID from where_am_i or list_surfaces");
const PANE: Argument = Argument::required("pane_id", "Pane ID from where_am_i or list_surfaces");
const TAB: Argument = Argument::required("tab_id", "Tab ID from where_am_i or list_surfaces");
const CWD: Argument = Argument::optional("cwd", "Working directory");
const START_PEER: &[Argument] = &[
    PANE,
    Argument::required("program", "Peer CLI executable"),
    Argument {
        name: "args",
        description: "Arguments to the peer CLI in order",
        required: false,
        kind: ArgumentKind::StringArray,
    },
    CWD,
    Argument::optional("prompt", "Initial prompt to pass to the peer CLI"),
];
const SEND_KEY: &[Argument] = &[
    PANE,
    Argument::required("key", "Terminal key name"),
    Argument {
        name: "modifiers",
        description: "Modifiers of this keystroke",
        required: false,
        kind: ArgumentKind::ChoiceArray(&["ctrl", "alt", "shift"]),
    },
];

impl Tool {
    pub const ALL: [Self; 8] = [
        Self::WhereAmI,
        Self::ListSurfaces,
        Self::CreateTab,
        Self::SplitPane,
        Self::StartPeer,
        Self::SendKey,
        Self::ClosePane,
        Self::CloseTab,
    ];

    pub const fn name(self) -> &'static str {
        self.spec().0
    }

    pub(crate) fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|tool| tool.name() == name)
    }

    const fn spec(self) -> (&'static str, &'static str, &'static [Argument]) {
        match self {
            Self::WhereAmI => (
                "where_am_i",
                "Identify the caller's instance, window, tab group, tab and pane, and its block when known.",
                &[],
            ),
            Self::ListSurfaces => (
                "list_surfaces",
                "List addressable windows, tabs and panels in the attached warp-ish profile.",
                &[],
            ),
            Self::CreateTab => (
                "create_tab",
                "Create a terminal tab in the addressed window.",
                &[WINDOW],
            ),
            Self::SplitPane => (
                "split_pane",
                "Add a split beside the addressed pane and return the new pane ID.",
                &[
                    PANE,
                    Argument {
                        name: "direction",
                        description: "Side on which the new pane opens",
                        required: true,
                        kind: ArgumentKind::Choice(&["left", "right", "up", "down"]),
                    },
                ],
            ),
            Self::StartPeer => (
                "start_peer",
                "Run a local peer CLI in the addressed ready pane. Pass each argument separately.",
                START_PEER,
            ),
            Self::SendKey => (
                "send_key",
                "Send one terminal key to an addressed pane.",
                SEND_KEY,
            ),
            Self::ClosePane => (
                "close_pane",
                "Close the addressed pane using the normal terminal close behavior.",
                &[PANE],
            ),
            Self::CloseTab => (
                "close_tab",
                "Close the addressed tab using the normal terminal close behavior.",
                &[TAB],
            ),
        }
    }

    pub(crate) fn definition(self) -> Value {
        let (name, description, arguments) = self.spec();
        let properties: Map<_, _> = arguments
            .iter()
            .map(|arg| (arg.name.into(), arg.definition()))
            .collect();
        let required: Vec<_> = arguments
            .iter()
            .filter(|arg| arg.required)
            .map(|arg| arg.name)
            .collect();
        json!({
            "name": name,
            "description": description,
            "inputSchema": {
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false
            }
        })
    }

    pub(crate) fn validate(self, values: &Map<String, Value>) -> Result<(), String> {
        let (_, _, arguments) = self.spec();
        if let Some(unknown) = values
            .keys()
            .find(|name| !arguments.iter().any(|arg| arg.name == name.as_str()))
        {
            return Err(format!("Unknown argument: {unknown}"));
        }
        for argument in arguments {
            match values.get(argument.name) {
                Some(value) if !argument.kind.accepts(value) => {
                    return Err(format!("Invalid argument: {}", argument.name));
                }
                None if argument.required => {
                    return Err(format!("Missing argument: {}", argument.name));
                }
                _ => {}
            }
        }
        Ok(())
    }
}

struct Argument {
    name: &'static str,
    description: &'static str,
    required: bool,
    kind: ArgumentKind,
}

impl Argument {
    const fn required(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            required: true,
            kind: ArgumentKind::String,
        }
    }

    const fn optional(name: &'static str, description: &'static str) -> Self {
        Self {
            name,
            description,
            required: false,
            kind: ArgumentKind::String,
        }
    }

    fn definition(&self) -> Value {
        let mut schema = match self.kind {
            ArgumentKind::String => json!({"type": "string", "minLength": 1}),
            ArgumentKind::StringArray => json!({"type": "array", "items": {"type": "string"}}),
            ArgumentKind::Choice(choices) => json!({"type": "string", "enum": choices}),
            ArgumentKind::ChoiceArray(choices) => {
                json!({"type": "array", "items": {"type": "string", "enum": choices}})
            }
        };
        schema["description"] = json!(self.description);
        schema
    }
}

enum ArgumentKind {
    String,
    StringArray,
    Choice(&'static [&'static str]),
    ChoiceArray(&'static [&'static str]),
}

impl ArgumentKind {
    fn accepts(&self, value: &Value) -> bool {
        match *self {
            Self::String => value
                .as_str()
                .is_some_and(|text| !text.is_empty() && !text.contains('\0')),
            Self::StringArray => value.as_array().is_some_and(|items| {
                items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|s| !s.contains('\0')))
            }),
            Self::Choice(choices) => value.as_str().is_some_and(|text| choices.contains(&text)),
            Self::ChoiceArray(choices) => value.as_array().is_some_and(|items| {
                items
                    .iter()
                    .all(|item| item.as_str().is_some_and(|s| choices.contains(&s)))
            }),
        }
    }
}
