use zellij_tile::prelude::*;

use std::collections::{HashMap, BTreeMap};

#[derive(Debug, Default)]
struct State {
    filter: String,
    tabs: Vec<String>,
    panes: HashMap<PaneIdHashable, String>, // String -> pane title
    current_matches: Vec<Match>,
    selected_tab_index: Option<usize>,
    selected_match_index: Option<usize>,
}

#[derive(Debug)]
pub struct Match {
    pub pane_id: PaneId,
    pub text: Text,
    pub selected_for_extraction: bool,
}

impl Match {
    pub fn new(pane_id: PaneId, text: Text) -> Self {
        Match {
            pane_id,
            text,
            selected_for_extraction: false
        }
    }
    pub fn toggle_mark_for_extraction(&mut self) {
        self.selected_for_extraction = !self.selected_for_extraction;
    }
}

register_plugin!(State);

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
pub struct PaneIdHashable {
    pub pane_id: u32,
    pub is_plugin: bool
}

impl Into<PaneId> for &PaneIdHashable {
    fn into(self) -> PaneId {
        if self.is_plugin {
            PaneId::Plugin(self.pane_id)
        } else {
            PaneId::Terminal(self.pane_id)
        }
    }
}

impl PaneIdHashable {
    pub fn plugin(pane_id: u32) -> Self {
        PaneIdHashable {
            pane_id,
            is_plugin: true
        }
    }
    pub fn terminal(pane_id: u32) -> Self {
        PaneIdHashable {
            pane_id,
            is_plugin: false
        }
    }
}

impl ZellijPlugin for State {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        request_permission(&[PermissionType::ReadApplicationState, PermissionType::ChangeApplicationState]);
        subscribe(&[EventType::ModeUpdate, EventType::TabUpdate, EventType::PaneUpdate, EventType::Key]);
    }
    fn update(&mut self, event: Event) -> bool {
        // TODO:
        // * implement a self.filter input
        // * when pressing ENTER it should transfer all tabs in the filter to the selected tab
        // * display the current tabs in a tab-bar ribbon, allowing to page through them with tab
        // for the destination tab
        // * once this works, commit the API
        // * work with this a little bit, see how it works and what else we want to add
        let mut should_render = false;
        match event {
            Event::ModeUpdate(mode_info) => {
            }
            Event::TabUpdate(tab_info) => {
                let previous_tabs = self.tabs.clone();
                self.tabs = tab_info.iter().map(|t| t.name.clone()).collect();
                if previous_tabs != self.tabs {
                    self.selected_tab_index = None;
                }
                should_render = true;
            }
            Event::PaneUpdate(pane_manifest) => {
                self.panes.clear();
                for (tab_index, panes) in pane_manifest.panes {
                    for pane_info in panes {
                        if !pane_info.is_selectable {
                            // we don't want to log "UI" panes
                            continue;
                        }
                        if pane_info.is_plugin {
                            self.panes.insert(PaneIdHashable::plugin(pane_info.id), pane_info.title);
                        } else {
                            self.panes.insert(PaneIdHashable::terminal(pane_info.id), pane_info.title);
                        }
                    }
                }
            }
            Event::Key(key) => {
                match key.bare_key {
                    BareKey::Char(character) if key.has_no_modifiers() => {
                        self.filter.push(character);
                        self.trigger_search();
                        should_render = true;
                    },
                    BareKey::Backspace if key.has_no_modifiers() => {
                        self.filter.pop();
                        if !self.filter.is_empty() {
                            self.trigger_search();
                        } else {
                            self.clear_search();
                        }
                        should_render = true;
                    }
                    BareKey::Enter if key.has_no_modifiers() => {
                        if !self.current_matches.is_empty() {
                            let should_focus = false;
                            match self.selected_tab_index {
                                None => {
                                    let new_tab_name = &self.filter;
                                    break_panes_to_new_tab(
                                        &self.panes_to_extract(),
                                        Some(new_tab_name.to_owned()),
                                        should_focus,
                                    );
                                    self.clear_search();
                                },
                                Some(tab_index) => {
                                    break_panes_to_tab_with_index(
                                        &self.panes_to_extract(),
                                        tab_index,
                                        should_focus,
                                    );
                                    self.clear_search();
                                }
                            }
                        }
                        should_render = true;
                    }
                    BareKey::Tab if key.has_no_modifiers() => {
                        if self.selected_tab_index.is_none() && !self.tabs.is_empty() {
                            self.selected_tab_index = Some(0);
                        } else if self.selected_tab_index == Some(self.tabs.len().saturating_sub(1)) {
                            self.selected_tab_index = None;
                        } else {
                            self.selected_tab_index = self.selected_tab_index.as_mut().map(|i| *i + 1);
                        }
                        should_render = true;
                    }
                    BareKey::Down if key.has_no_modifiers() => {
                        if self.selected_match_index.is_none() && !self.current_matches.is_empty() {
                            self.selected_match_index = Some(0);
                        } else if self.selected_match_index == Some(self.current_matches.len().saturating_sub(1)) {
                            self.selected_match_index = None;
                        } else {
                            self.selected_match_index = self.selected_match_index.as_mut().map(|i| *i + 1);
                        }
                        should_render = true;
                    }
                    BareKey::Up if key.has_no_modifiers() => {
                        if self.selected_match_index.is_none() && !self.current_matches.is_empty() {
                            self.selected_match_index = Some(self.current_matches.len().saturating_sub(1));
                        } else if self.selected_match_index == Some(0) {
                            self.selected_match_index = None;
                        } else {
                            self.selected_match_index = self.selected_match_index.as_mut().map(|i| i.saturating_sub(1));
                        }
                        should_render = true;
                    }
                    BareKey::Right | BareKey::Left if key.has_no_modifiers() => {
                        self.toggle_mark_selected_for_extraction();
                        should_render = true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        should_render
    }

    fn render(&mut self, rows: usize, cols: usize) {
        // TODO:
        // * break these into compoonent
        // * work on tab line responsiveness
        let rows_for_search_line = 1;
        let rows_for_control_line = 1;
        let rows_for_move_to_line = 1;
        let gap_row_count = 2;
        let rows_for_table = rows
            .saturating_sub(rows_for_search_line)
            .saturating_sub(rows_for_control_line)
            .saturating_sub(rows_for_move_to_line)
            .saturating_sub(gap_row_count);
        let max_width = Some(cols);
        // SEARCH LINE
        let prompt = "SEARCH PANES:";
        let search_line = Text::new(format!("{} {}_", prompt, self.filter));
        print_text_with_coordinates(search_line, 0, 0, max_width, Some(rows_for_search_line));

        // RESULTS TABLE
        let mut current_matches_table = Table::new()
            .add_row(vec![" ", " ", " "]);
        let panes_selected_for_extraction = self.current_matches.iter().filter(|m| if m.selected_for_extraction { true } else { false }).count();
        let mut first_row_index = self.selected_match_index.map(|s_i| s_i.saturating_sub(rows_for_table / 2)).unwrap_or(0);
        let last_row_index = (first_row_index + rows_for_table).saturating_sub(2); // 1 for the
                                                                                   // title row, 1
                                                                                   // for count ->
                                                                                   // index
        if last_row_index > self.current_matches.len() {
            first_row_index = first_row_index.saturating_sub(last_row_index - self.current_matches.len());
        }
        let rows_above = first_row_index;
        let rows_below = self.current_matches.iter().count().saturating_sub(last_row_index);
        for (i, current_match) in self.current_matches.iter().enumerate().skip(first_row_index).take(rows_for_table.saturating_sub(1)) {
            let mut row = vec![];
            row.append(&mut vec![Text::new(">"), current_match.text.clone()]);
            if i == first_row_index && rows_above > 0 {
                row.push(Text::new(format!("[+ {}]", rows_above)).color_range(2, ..));
            } else if i == last_row_index && rows_below > 0 {
                row.push(Text::new(format!("[+ {}]", rows_below)).color_range(2, ..));
            } else {
                row.push(Text::new(format!(" ")));
            }
            if current_match.selected_for_extraction {
                for item in row.iter_mut() {
                    *item = item.clone().color_range(0, ..);
                }
            }
            if self.selected_match_index == Some(i) {
                for item in row.iter_mut() {
                    *item = item.clone().selected();
                }
            }
            current_matches_table = current_matches_table.add_styled_row(row);
        }
        print_table_with_coordinates(current_matches_table, 0, 1, Some(cols), None);

        // MOVE TO LINE
        let move_to_text = "Move to:";
        let tab_line_y = rows.saturating_sub(3);
        print_text_with_coordinates(Text::new(move_to_text), 0, tab_line_y, None, None);
        let tab_toggle_indication = "<TAB>";
        print_text_with_coordinates(Text::new(tab_toggle_indication).color_range(3, ..), 9, tab_line_y, None, None);
        let mut tab_x = 9 + 6;
        for (i, tab) in self.tabs.iter().enumerate() {
            if self.selected_tab_index == Some(i) {
                print_ribbon_with_coordinates(Text::new(tab).selected(), tab_x, tab_line_y, None, None);
            } else {
                print_ribbon_with_coordinates(Text::new(tab), tab_x, tab_line_y, None, None);
            }
            tab_x += tab.chars().count() + 4;
        }
        if self.selected_tab_index.is_none() {
            print_ribbon_with_coordinates(Text::new("[NEW TAB]").selected(), tab_x, tab_line_y, None, None);
        } else {
            print_ribbon_with_coordinates(Text::new("[NEW TAB]"), tab_x, tab_line_y, None, None);
        }


        // CONTROLS LINE
        let enter_text = "<ENTER>";
        let enter_legend = if panes_selected_for_extraction > 0 {
            format!("Move {} selected panes to new tab", panes_selected_for_extraction)
        } else {
            "Move panes to selected tab".to_owned()
        };
        let arrows_text = "<←↓↑→>";
        let arrows_legend = "Navigate and select entries";
        let arrow_legend_start_pos = enter_text.len() + enter_legend.len() + 5; // 5 is the spaces
        let arrow_legend_end_pos = arrow_legend_start_pos + arrows_text.chars().count();
        let controls_line_y = rows;
        let text = if panes_selected_for_extraction > 0 {
            let pane_count_start_pos = enter_text.chars().count() + 8;
            let pane_count_end_pos = pane_count_start_pos + format!("{}", panes_selected_for_extraction).chars().count();
            Text::new(format!("{} - {}, {} - {}", enter_text, enter_legend, arrows_text, arrows_legend))
                .color_range(3, ..enter_text.len())
                .color_range(0, pane_count_start_pos..pane_count_end_pos)
                .color_range(3, arrow_legend_start_pos..arrow_legend_end_pos)
        } else {
            Text::new(format!("{} - {}, {} - {}", enter_text, enter_legend, arrows_text, arrows_legend))
                .color_range(3, ..enter_text.len())
                .color_range(3, arrow_legend_start_pos..arrow_legend_end_pos)
        };
        print_text_with_coordinates(
            text,
            0,
            controls_line_y,
            None,
            None
        );

        // TODO:
        // * move to functionality - DONE
        // * rename the new tab to the search filter - DONE
        // * page through results and select individiaul ones to move - DONE
        // * UI / responsiveness <=== CONTINUE HERE
    }
}

impl State {
    pub fn trigger_search(&mut self) {
        self.current_matches.clear();
        let filter_len = self.filter.chars().count();
        let lc_filter = self.filter.to_lowercase();
        for (pane_id, pane_title) in &self.panes {
            let lc_pane_title = pane_title.to_lowercase();
            let matches = lc_pane_title.match_indices(&lc_filter).collect::<Vec<_>>();
            if !matches.is_empty() {
                let mut text = Text::new(pane_title);
                for (match_index, _) in matches {
                    text = text.color_range(3, match_index..match_index + filter_len);
                }
                self.current_matches.push(Match::new(pane_id.into(), text));
            }
        }
    }
    pub fn clear_search(&mut self) {
        self.filter.clear();
        self.current_matches.clear();
        self.selected_match_index = None;
    }
    pub fn toggle_mark_selected_for_extraction(&mut self) {
        if let Some(index) = self.selected_match_index {
            self.current_matches.get_mut(index).map(|m| m.toggle_mark_for_extraction());
        }
    }
    pub fn panes_to_extract(&self) -> Vec<PaneId> {
        let pane_ids_selected_for_extraction = self.current_matches.iter().filter_map(|m| if m.selected_for_extraction { Some(m.pane_id) } else { None }).collect::<Vec<_>>();
        if pane_ids_selected_for_extraction.is_empty() {
            // if nothing is selected, we take everything
            self.current_matches.iter().map(|m| m.pane_id).collect::<Vec<_>>()
        } else {
            pane_ids_selected_for_extraction
        }
    }
}
