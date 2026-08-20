use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;

pub use crate::ecel::{parse_condition, Condition};

pub type NodeId = String;
pub type GateId = String;

#[derive(Debug, Clone, Serialize)]
pub struct EtlDocument {
    pub etdl: String,
    pub info: Info,
    #[serde(default)]
    pub asyncapi_imports: BTreeMap<String, String>,

    #[serde(default)]
    pub supplements: Vec<Supplement>,

    /// Declared ETDL Standard Library / domain / optional / user library
    /// imports (see `etdl-compiler::stdlib`). Distinct from `supplements`:
    /// a supplement is a compiled-in Rust extension identified by id; a
    /// library is (typically) ETDL source resolved and merged before the
    /// rest of the pipeline runs, so nothing downstream of parsing needs to
    /// know libraries exist.
    #[serde(default)]
    pub libraries: Vec<LibraryImport>,

    #[serde(default)]
    pub components: Option<Components>,

    pub event_trees: BTreeMap<String, EventTree>,

    #[serde(default)]
    pub fault_trees: Option<BTreeMap<String, FaultTree>>,

    pub extensions: BTreeMap<String, serde_yaml::Value>,
}

/// A declared supplement/extension (core Section 5.1.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Supplement {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub required: bool,
}

/// A declared library import: `{ name: "std.events", version: "1.0" }`.
///
/// Mirrors [`Supplement`]'s shape and required/optional semantics
/// deliberately: both are "a named, versioned external capability declared
/// in the document" — a supplement resolves to a compiled-in Rust extension,
/// a library resolves to ETDL source (built-in, optional, or user-provided).
/// See `etdl-compiler::stdlib` for resolution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryImport {
    /// The library's dotted name, e.g. `std.events`. Names starting with
    /// `std.` are reserved for the built-in standard library and can never
    /// resolve to an optional or user library (see `stdlib::LibraryError::Shadowing`).
    pub name: String,
    /// The requested library version, e.g. `"1.0"`. Compatibility is
    /// major-version-gated, the same rule already used for `doc.etdl` and
    /// for `Supplement::version`.
    pub version: String,
    #[serde(default)]
    pub required: bool,
}

impl<'de> Deserialize<'de> for EtlDocument {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::{Error, MapAccess, Visitor};
        use std::fmt;

        struct DocVisitor;

        impl<'de> Visitor<'de> for DocVisitor {
            type Value = EtlDocument;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an ETDL document")
            }

            fn visit_map<A>(self, mut map: A) -> Result<EtlDocument, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut etdl: Option<String> = None;
                let mut info: Option<Info> = None;
                let mut asyncapi_imports: BTreeMap<String, String> = BTreeMap::new();
                let mut supplements: Vec<Supplement> = Vec::new();
                let mut libraries: Vec<LibraryImport> = Vec::new();
                let mut components: Option<Components> = None;
                let mut event_trees_map: BTreeMap<String, EventTree> = BTreeMap::new();
                let mut event_tree_legacy: Option<EventTree> = None;
                let mut fault_trees: Option<BTreeMap<String, FaultTree>> = None;
                let mut extensions: BTreeMap<String, serde_yaml::Value> = BTreeMap::new();

                while let Some(key) = map.next_key::<String>()? {
                    match key.as_str() {
                        "etdl" => {
                            if etdl.is_some() {
                                return Err(Error::duplicate_field("etdl"));
                            }
                            etdl = Some(map.next_value()?);
                        }
                        "info" => {
                            if info.is_some() {
                                return Err(Error::duplicate_field("info"));
                            }
                            info = Some(map.next_value()?);
                        }
                        "asyncapi_imports" => {
                            asyncapi_imports = map.next_value()?;
                        }
                        "supplements" => {
                            supplements = map.next_value()?;
                        }
                        "libraries" => {
                            libraries = map.next_value()?;
                        }
                        "components" => {
                            components = map.next_value()?;
                        }
                        "eventTrees" => {
                            event_trees_map = map.next_value()?;
                        }
                        "eventTree" => {
                            event_tree_legacy = Some(map.next_value()?);
                        }
                        "faultTrees" => {
                            fault_trees = map.next_value()?;
                        }
                        k if k.starts_with("x-") => {
                            let val: serde_yaml::Value = map.next_value()?;
                            extensions.insert(k.to_string(), val);
                        }
                        unknown => {
                            return Err(Error::custom(format!(
                                "unrecognized field '{}' in ETDL document; extension fields must start with 'x-'",
                                unknown
                            )));
                        }
                    }
                }

                let etdl = etdl.ok_or_else(|| Error::missing_field("etdl"))?;
                let info = info.ok_or_else(|| Error::missing_field("info"))?;

                let event_trees = match (event_trees_map.is_empty(), event_tree_legacy) {
                    (true, Some(tree)) => {
                        let mut map = BTreeMap::new();
                        map.insert("default".to_string(), tree);
                        map
                    }
                    (true, None) => {
                        return Err(Error::custom(
                            "at least one of 'eventTrees' or 'eventTree' (deprecated) must be present",
                        ));
                    }
                    (false, None) => event_trees_map,
                    (false, Some(_)) => {
                        return Err(Error::custom(
                            "both 'eventTrees' and 'eventTree' (deprecated) provided; use only 'eventTrees'",
                        ));
                    }
                };

                Ok(EtlDocument {
                    etdl,
                    info,
                    asyncapi_imports,
                    supplements,
                    libraries,
                    components,
                    event_trees,
                    fault_trees,
                    extensions,
                })
            }
        }

        deserializer.deserialize_map(DocVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Info {
    pub title: String,
    pub version: String,
    #[serde(deserialize_with = "deserialize_domain")]
    pub domain: String,
    #[serde(default)]
    pub description: Option<String>,
}

fn deserialize_domain<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    if s.is_empty() || !s.chars().next().unwrap().is_ascii_alphabetic() {
        return Err(serde::de::Error::custom(
            "domain must match ^[A-Za-z][A-Za-z0-9]*$",
        ));
    }
    if s.chars().any(|c| !c.is_ascii_alphanumeric()) {
        return Err(serde::de::Error::custom(
            "domain must match ^[A-Za-z][A-Za-z0-9]*$",
        ));
    }
    Ok(s)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Components {
    #[serde(default)]
    pub barriers: Option<BTreeMap<String, Barrier>>,
    #[serde(default)]
    pub operations: Option<BTreeMap<String, Operation>>,
    #[serde(default)]
    pub gates: Option<BTreeMap<String, Gate>>,
    #[serde(default)]
    pub basic_events: Option<BTreeMap<String, BasicEvent>>,
    /// Inline Message Schema Objects (Section 5.4.1), resolved by a Message
    /// Reference of the form `#/components/messages/<id>` (Section 5.3.4).
    #[serde(default)]
    pub messages: Option<BTreeMap<String, Message>>,
}

/// A Message Schema Object (Section 5.4.1): an inline, AsyncAPI 3.0 Message
/// Object-shaped definition, used when a document has no `asyncapi_imports`
/// (or a specific message isn't covered by one) and instead defines its
/// message shape directly under `components.messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    #[serde(default)]
    pub name: Option<String>,
    /// A JSON Schema describing the message payload.
    pub payload: serde_yaml::Value,
    /// A JSON Schema describing the message headers.
    #[serde(default)]
    pub headers: Option<serde_yaml::Value>,
}

/// An ETDL **library document**: a reusable component catalog (the standard
/// library, a domain library, or a user library), as opposed to an
/// [`EtlDocument`] (a system: event trees, fault trees, an actual model).
///
/// A library has no event trees or fault trees of its own — it only
/// *provides* named, reusable `components` that an importing [`EtlDocument`]
/// can reference. It is parsed with the same YAML conventions and the exact
/// same [`Components`]/[`BasicEvent`]/[`Gate`] types as an ordinary document
/// (see `etdl_parser::parse_library_document`), just under a lighter
/// top-level schema that does not require an event tree to be present.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDocument {
    /// The ETDL language-version dialect this library's syntax targets.
    /// Checked the same way as [`EtlDocument::etdl`].
    pub etdl: String,
    pub library: LibraryInfo,
    #[serde(default)]
    pub components: Components,
}

/// A library's own identity: name, version, and description. `version` is a
/// distinct axis from `etdl` (the language dialect) and from any crate
/// version — see `docs/reference/standard-library.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryInfo {
    /// The library's dotted name, e.g. `std.events`. Must match the `name`
    /// an importing document declares in `libraries:`.
    pub name: String,
    /// The library's own version, e.g. `"1.0"`.
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Other libraries this one depends on. Resolved transitively with
    /// cycle detection (`stdlib::LibraryError::Cyclic`); kept simple
    /// deliberately (no version solver, no diamond-dependency merge logic).
    #[serde(default, rename = "dependsOn")]
    pub depends_on: Vec<LibraryImport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTree {
    #[serde(rename = "initiatingEvent")]
    pub initiating_event: InitiatingEvent,
    pub nodes: BTreeMap<NodeId, Node>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiatingEvent {
    pub id: String,
    pub message: MessageRef,
    pub next: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Node {
    #[serde(rename = "barrier")]
    Barrier(Barrier),
    #[serde(rename = "operation")]
    Operation(Operation),
    #[serde(rename = "consequence")]
    Consequence(Consequence),
}

impl Node {
    pub fn node_type(&self) -> &str {
        match self {
            Node::Barrier(_) => "barrier",
            Node::Operation(_) => "operation",
            Node::Consequence(_) => "consequence",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Barrier {
    pub branches: Vec<Branch>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub outcome: String,

    #[serde(deserialize_with = "deserialize_condition")]
    pub condition: Condition,

    #[serde(default, deserialize_with = "deserialize_optional_f64")]
    pub probability: Option<f64>,

    #[serde(default, alias = "probabilityOfSuccess")]
    pub probability_of_success: Option<f64>,

    #[serde(default, alias = "probabilityOfFailure")]
    pub probability_of_failure: Option<f64>,

    #[serde(default, alias = "probabilitySource")]
    pub probability_source: Option<InternalRef>,

    pub next: NodeId,
}

fn deserialize_condition<'de, D>(deserializer: D) -> Result<Condition, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    parse_condition(&s).map_err(serde::de::Error::custom)
}

fn deserialize_optional_f64<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<f64>::deserialize(deserializer)
}

impl Branch {
    pub fn effective_probability(&self) -> Option<f64> {
        self.probability
            .or(self.probability_of_success)
            .or(self.probability_of_failure)
    }

    pub fn has_probability_source(&self) -> bool {
        self.probability_source.is_some()
            || self.probability.is_some()
            || self.probability_of_success.is_some()
            || self.probability_of_failure.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Operation {
    #[serde(default = "default_action")]
    pub action: ActionKind,
    pub handler: String,

    #[serde(default)]
    pub emits: Option<MessageRef>,

    pub next: NodeId,

    #[serde(default, alias = "onFailure")]
    pub on_failure: Option<NodeId>,

    #[serde(default, alias = "onFailureProbabilitySource")]
    pub on_failure_probability_source: Option<InternalRef>,

    #[serde(default, alias = "retryPolicy")]
    pub retry_policy: Option<RetryPolicy>,

    #[serde(default, alias = "timeoutMs")]
    pub timeout_ms: Option<u64>,

    #[serde(default)]
    pub description: Option<String>,
}

fn default_action() -> ActionKind {
    ActionKind::Execute
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ActionKind {
    #[serde(rename = "execute")]
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryPolicy {
    #[serde(default = "default_max_attempts", alias = "maxAttempts")]
    pub max_attempts: u32,

    #[serde(default = "default_backoff_ms", alias = "backoffMs")]
    pub backoff_ms: u64,

    #[serde(default, alias = "backoffStrategy")]
    pub backoff_strategy: Option<BackoffStrategy>,
}

fn default_max_attempts() -> u32 {
    1
}
fn default_backoff_ms() -> u64 {
    0
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub enum BackoffStrategy {
    #[serde(rename = "fixed")]
    #[default]
    Fixed,
    #[serde(rename = "exponential")]
    Exponential,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Consequence {
    #[serde(rename = "operation")]
    pub consequence_operation: ConsequenceOperation,
    #[serde(default)]
    pub channel: Option<ChannelRef>,
    #[serde(default)]
    pub message: Option<MessageRef>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConsequenceOperation {
    #[serde(rename = "send")]
    Send,
    #[serde(rename = "terminate")]
    Terminate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FaultTree {
    #[serde(rename = "topEvent")]
    pub top_event: TopEvent,

    #[serde(default)]
    pub gates: Option<BTreeMap<GateId, Gate>>,

    #[serde(rename = "basicEvents")]
    pub basic_events: BTreeMap<String, BasicEvent>,

    #[serde(default)]
    pub transfers: Option<BTreeMap<String, TransferNode>>,

    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopEvent {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub message: Option<MessageRef>,
    #[serde(rename = "rootCause")]
    pub root_cause: FaultTreeNodeRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    #[serde(rename = "type")]
    pub gate_type: GateType,
    pub inputs: Vec<FaultTreeNodeRef>,
    #[serde(default)]
    pub k: Option<u32>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, alias = "inhibitCondition")]
    pub inhibit_condition: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GateType {
    #[serde(rename = "AND")]
    And,
    #[serde(rename = "OR")]
    Or,
    #[serde(rename = "NOT")]
    Not,
    #[serde(rename = "XOR")]
    Xor,
    #[serde(rename = "VOTING")]
    Voting,
    #[serde(rename = "INHIBIT")]
    Inhibit,
    #[serde(rename = "PRIORITY_AND")]
    PriorityAnd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BasicEventType {
    #[serde(rename = "basic")]
    Basic,
    #[serde(rename = "house")]
    House,
    #[serde(rename = "undeveloped")]
    Undeveloped,
    #[serde(rename = "conditional")]
    Conditional,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferNode {
    pub target: String,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicEvent {
    pub description: String,
    #[serde(default)]
    pub probability: Option<f64>,
    #[serde(default, alias = "failureRate")]
    pub failure_rate: Option<f64>,
    #[serde(default, alias = "missionTime")]
    pub mission_time: Option<f64>,
    #[serde(default)]
    pub undeveloped: Option<bool>,
    #[serde(default, alias = "eventType")]
    pub event_type: Option<BasicEventType>,
    #[serde(default)]
    pub message: Option<MessageRef>,
    /// `x-*` extension fields (core Section 11), preserved as raw values.
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Clone)]
pub struct ExternalRef {
    pub alias: String,
    pub pointer: String,
}

#[derive(Debug, Clone)]
pub struct InternalRef {
    pub pointer: String,
}

pub type FaultTreeNodeRef = String;

impl Serialize for ExternalRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&format!("{}#{}", self.alias, self.pointer))
    }
}

impl<'de> Deserialize<'de> for ExternalRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_external_ref(&s).map_err(serde::de::Error::custom)
    }
}

fn parse_external_ref(s: &str) -> Result<ExternalRef, String> {
    if let Some(hash_pos) = s.find('#') {
        let alias = &s[..hash_pos];
        let pointer = &s[hash_pos..];
        if alias.is_empty() {
            if pointer.starts_with("#/") {
                return Err(format!(
                    "bare JSON Pointer '{}' without import alias; use InternalRef for same-document references",
                    pointer
                ));
            }
            return Err("empty alias in external reference".to_string());
        }
        if alias
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            return Err(format!("invalid import alias '{}'", alias));
        }
        Ok(ExternalRef {
            alias: alias.to_string(),
            pointer: pointer.to_string(),
        })
    } else {
        Err(format!("no '#' found in external reference '{}'", s))
    }
}

impl Serialize for InternalRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.pointer)
    }
}

impl<'de> Deserialize<'de> for InternalRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if !s.starts_with("#/") {
            return Err(serde::de::Error::custom(format!(
                "invalid internal reference '{}'; must start with '#/'",
                s
            )));
        }
        Ok(InternalRef { pointer: s })
    }
}

impl ExternalRef {
    pub fn as_string(&self) -> String {
        format!("{}#{}", self.alias, self.pointer)
    }
}

impl InternalRef {
    pub fn as_string(&self) -> String {
        self.pointer.clone()
    }
}

#[derive(Debug, Clone)]
pub enum ParsedReference {
    External(ExternalRef),
    Internal(InternalRef),
}

pub fn parse_reference(s: &str) -> Result<ParsedReference, String> {
    if s.starts_with("#/") {
        Ok(ParsedReference::Internal(InternalRef {
            pointer: s.to_string(),
        }))
    } else if let Some(hash_pos) = s.find('#') {
        let alias = &s[..hash_pos];
        let pointer = &s[hash_pos..];
        if alias.is_empty() || pointer.is_empty() || !pointer.starts_with('#') {
            return Err(format!("invalid reference syntax: '{}'", s));
        }
        parse_external_ref(s).map(ParsedReference::External)
    } else {
        Err(format!(
            "reference '{}' matches neither external nor internal format",
            s
        ))
    }
}

/// A Message Reference (Section 5.3.4): either an External Reference into a
/// loaded `asyncapi_imports` document, or an Internal Reference of the form
/// `#/components/messages/<id>` resolving to an inline Message Schema
/// Object (Section 5.4.1).
#[derive(Debug, Clone)]
pub enum MessageRef {
    External(ExternalRef),
    Internal(InternalRef),
}

impl Serialize for MessageRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            MessageRef::External(r) => r.serialize(serializer),
            MessageRef::Internal(r) => r.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for MessageRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match parse_reference(&s).map_err(serde::de::Error::custom)? {
            ParsedReference::External(r) => Ok(MessageRef::External(r)),
            ParsedReference::Internal(r) => Ok(MessageRef::Internal(r)),
        }
    }
}

impl MessageRef {
    pub fn as_string(&self) -> String {
        match self {
            MessageRef::External(r) => r.as_string(),
            MessageRef::Internal(r) => r.as_string(),
        }
    }
}

/// A Channel Reference (Section 5.3.5): an External Reference (required
/// whenever the document declares any `asyncapi_imports`), or — only when
/// the document has no `asyncapi_imports` at all — a bare channel-name
/// string. Never an Internal Reference: channel addressing has no inline
/// schema counterpart to `components.messages`.
#[derive(Debug, Clone)]
pub enum ChannelRef {
    External(ExternalRef),
    Bare(String),
}

impl Serialize for ChannelRef {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ChannelRef::External(r) => r.serialize(serializer),
            ChannelRef::Bare(s) => serializer.serialize_str(s),
        }
    }
}

impl<'de> Deserialize<'de> for ChannelRef {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if s.starts_with("#/") {
            return Err(serde::de::Error::custom(format!(
                "'{}' is not a valid Channel Reference: internal pointers are not supported for channel (Section 5.3.5)",
                s
            )));
        }
        if s.contains('#') {
            parse_external_ref(&s)
                .map(ChannelRef::External)
                .map_err(serde::de::Error::custom)
        } else {
            Ok(ChannelRef::Bare(s))
        }
    }
}

impl ChannelRef {
    pub fn as_string(&self) -> String {
        match self {
            ChannelRef::External(r) => r.as_string(),
            ChannelRef::Bare(s) => s.clone(),
        }
    }
}
