use super::lock_unpoisoned;
use std::collections::BTreeMap;
use std::fs;

use ignore::WalkBuilder;

use super::DesktopState;
use donder_sequence_api::{
    ProjectSearchMatch, ProjectSearchMatchKind, ProjectSearchRequest, ProjectSearchResponse,
};

const MAX_SEARCH_RESULTS: usize = 500;
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;

impl DesktopState {
    pub fn search_project(
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
            request.query.to_lowercase()
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
            let filename = if request.match_case {
                relative.clone()
            } else {
                relative.to_lowercase()
            };
            if let Some(column) = filename.find(&query) {
                push_match(
                    &mut response,
                    ProjectSearchMatch {
                        path: relative.clone(),
                        line: 0,
                        column: column as u32,
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
                let haystack = if request.match_case {
                    line.to_string()
                } else {
                    line.to_lowercase()
                };
                for (column, _) in haystack.match_indices(&query) {
                    push_match(
                        &mut response,
                        ProjectSearchMatch {
                            path: relative.clone(),
                            line: line_index as u32,
                            column: line[..column].chars().count() as u32,
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

fn push_match(response: &mut ProjectSearchResponse, item: ProjectSearchMatch) {
    if response.matches.len() == MAX_SEARCH_RESULTS {
        response.truncated = true;
    } else {
        response.matches.push(item);
    }
}
