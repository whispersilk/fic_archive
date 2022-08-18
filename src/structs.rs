pub struct Story {
    pub name: String,
    pub description: Option<String>,
    pub url: String,
    pub tags: Vec<String>,
    pub chapters: Vec<Content>,
}

pub enum Content {
    Section { name: String, description: Option<String>, chapters: Vec<Content> },
    Chapter { name: String, description: Option<String>, text: String },
}

