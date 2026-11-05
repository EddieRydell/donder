use super::lock_unpoisoned;
use std::collections::BTreeMap;
use std::fs;

use ignore::WalkBuilder;

use super::DesktopState;
use donder_editor::dto::{
    ProjectSearchMatch, ProjectSearchMatchKind, ProjectSearchRequest, ProjectSearchResponse,
};

const MAX_SEARCH_RESULTS: usize = 500;
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;

impl DesktopState {
    pub(crate) fn search_project(
        &self,
        request: ProjectSearchRequest,
    ) -> Result<ProjectSearchResponse, String> {
        let root = self
            .project_root_path()
            .ok_or_else(|| "No project is open.".to_string())?;
        let open_buffers = {
            let workspace = lock_unpoisoned(&self.workspace);
            workspace
                .tabs
                .iter()
                .filter_map(|path| workspace.documents.get(path))
                .map(|document| (document.buffer.path.clone(), document.buffer.text.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        let query = if request.match_case {
            request.query.clone()
        } else {
            request.query.chars().flat_map(char::to_lowercase).collect()
        };
        let mut response = ProjectSearchResponse {
            request_id: request.request_id,
            matches: Vec::new(),
            skipped_binary: 0,
            skipped_oversized: 0,
            truncated: false,
        };
        if query.is_empty() {
            return Ok(response);
        }

        for entry in WalkBuilder::new(&root)
            .standard_filters(true)
            .follow_links(false)
            .build()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
        {
            let Some(path) = camino::Utf8Path::from_path(entry.path()) else {
                continue;
            };
            let Ok(relative) = path.strip_prefix(&root) else {
                continue;
            };
            let relative = relative.as_str().replace('\\', "/");
            if let Some(column) = match_columns(&relative, &query, request.match_case)
                .first()
                .copied()
            {
                push_match(
                    &mut response,
                    ProjectSearchMatch {
                        path: relative.clone(),
                        line: 0,
                        column,
                        preview: relative.clone(),
                        kind: ProjectSearchMatchKind::Filename,
                    },
                );
            }
            if response.truncated {
                break;
            }

            let text = if let Some(text) = open_buffers.get(&relative) {
                text.clone()
            } else {
                let Ok(metadata) = entry.metadata() else {
                    continue;
                };
                if metadata.len() > MAX_SEARCH_FILE_BYTES {
                    response.skipped_oversized = response.skipped_oversized.saturating_add(1);
                    continue;
                }
                let Ok(bytes) = fs::read(path) else {
                    continue;
                };
                if bytes.contains(&0) {
                    response.skipped_binary = response.skipped_binary.saturating_add(1);
                    continue;
                }
                let Ok(text) = String::from_utf8(bytes) else {
                    response.skipped_binary = response.skipped_binary.saturating_add(1);
                    continue;
                };
                text
            };
            for (line_index, line) in text.lines().enumerate() {
                for column in match_columns(line, &query, request.match_case) {
                    push_match(
                        &mut response,
                        ProjectSearchMatch {
                            path: relative.clone(),
                            line: line_index as u32,
                            column,
                            preview: line.trim().to_string(),
                            kind: ProjectSearchMatchKind::Content,
                        },
                    );
                    if response.truncated {
                        break;
                    }
                }
                if response.truncated {
                    break;
                }
            }
            if response.truncated {
                break;
            }
        }
        Ok(response)
    }
}

/// The character column of each match of `query` in `text`. Lowercasing can
/// change a character's byte length, so a case-insensitive match is located in
/// the lowercased text and mapped back to the character it came from.
fn match_columns(text: &str, query: &str, match_case: bool) -> Vec<u32> {
    let mut folded = String::with_capacity(text.len());
    let mut origin = Vec::with_capacity(text.len());
    for (column, character) in text.chars().enumerate() {
        if match_case {
            folded.push(character);
        } else {
            folded.extend(character.to_lowercase());
        }
        origin.resize(folded.len(), column as u32);
    }
    folded
        .match_indices(query)
        .map(|(byte, _)| origin[byte])
        .collect()
}

fn push_match(response: &mut ProjectSearchResponse, item: ProjectSearchMatch) {
    if response.matches.len() == MAX_SEARCH_RESULTS {
        response.truncated = true;
    } else {
        response.matches.push(item);
    }
}
