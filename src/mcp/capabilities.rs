use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub resources: ResourcesCapabilities,
    pub prompts: PromptsCapabilities,
    pub tools: ToolsCapabilities,
    pub logging: LoggingCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourcesCapabilities {
    pub subscribe: bool,
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptsCapabilities {
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolsCapabilities {
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingCapabilities {}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            resources: ResourcesCapabilities {
                subscribe: false,
                list_changed: true,
            },
            prompts: PromptsCapabilities { list_changed: true },
            tools: ToolsCapabilities { list_changed: true },
            logging: LoggingCapabilities {},
        }
    }
}
