use crate::news::{Article, CATEGORIES};
use std::collections::HashSet;

#[derive(Default, Debug, PartialEq)]
pub enum Filter {
    #[default]
    All,
    Selected,
    Category(String),
}
impl Filter {
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "all" => Some(Self::All),
            "selected" => Some(Self::Selected),
            key if CATEGORIES.iter().any(|(id, _)| *id == key) => Some(Self::Category(key.into())),
            _ => None,
        }
    }
    pub fn visible<'a>(
        &self,
        articles: &'a [Article],
        selected: &HashSet<String>,
    ) -> Vec<&'a Article> {
        articles
            .iter()
            .filter(|a| match self {
                Self::All => true,
                Self::Selected => selected.contains(&a.id),
                Self::Category(key) => a.matches_category(key),
            })
            .collect()
    }
    pub fn toggle_all(&self, articles: &[Article], selected: &mut HashSet<String>) {
        let visible = self.visible(articles, selected);
        let all_selected = visible.iter().all(|a| selected.contains(&a.id));
        for article in visible {
            if all_selected {
                selected.remove(&article.id);
            } else {
                selected.insert(article.id.clone());
            }
        }
    }
    pub fn invert(&self, articles: &[Article], selected: &mut HashSet<String>) {
        for article in self.visible(articles, selected) {
            if !selected.remove(&article.id) {
                selected.insert(article.id.clone());
            }
        }
    }
}
