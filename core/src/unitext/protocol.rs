use std::collections::HashSet;
use std::time::Instant;

use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType, encoder};
use serde_json::Value;

use super::lut12;

pub(crate) const PLANE_COLS: usize = 32;
pub(crate) const WIDE_COLS: usize = 64;
pub(crate) const PAGE_ROWS: usize = 6;
pub(crate) const MIN_ROWS: usize = 4;
pub(crate) const DEFAULT_ROWS: usize = 6;
pub(crate) const MAX_ROWS: usize = 12;
pub(crate) const PLANE_CELLS: usize = PLANE_COLS * PAGE_ROWS;
pub(crate) const MAX_CELLS: usize = WIDE_COLS * MAX_ROWS;

pub(crate) const MIN_ROW_PX: usize = 192;
pub(crate) const DEFAULT_ROW_PX: usize = 384;
pub(crate) const MAX_ROW_PX: usize = 640;

pub(crate) const CTRL_NOP: u8 = 0;
pub(crate) const CTRL_UTF16_MIN: u8 = 1;
pub(crate) const CTRL_ASCII_MIN: u8 = 97;
pub(crate) const CTRL_LUT12_MIN: u8 = 145;
pub(crate) const CTRL_CLEAR_ALL: u8 = 240;
pub(crate) const CTRL_CLEAR_ROW: u8 = 241;
pub(crate) const CTRL_LUT12_LAST: u8 = 243;
pub(crate) const CTRL_PAGE_MIN: u8 = 244;
pub(crate) const CTRL_SET_VIEW: u8 = 250;
pub(crate) const CTRL_SET_GEOMETRY: u8 = 251;
pub(crate) const MIN_ROW_PX8: usize = MIN_ROW_PX / 8;
pub(crate) const MAX_ROW_PX8: usize = MAX_ROW_PX / 8;
pub(crate) const COMPACT36_BYTES: usize = 5;

fn full_bytes(n_bytes: usize) -> usize {
    if n_bytes == COMPACT36_BYTES {
        4
    } else {
        n_bytes
    }
}

fn mask_payload(data: &mut [u8], n_bytes: usize) {
    if n_bytes == COMPACT36_BYTES && data.len() > 4 {
        data[4] &= 0xF0;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UniTextTier {
    Lite,
    Compact,
    Balanced,
    Standard,
    Turbo,
}

impl UniTextTier {
    pub(crate) fn from_str_name(name: &str) -> Option<Self> {
        match name {
            "lite" => Some(Self::Lite),
            "compact" => Some(Self::Compact),
            "balanced" => Some(Self::Balanced),
            "standard" => Some(Self::Standard),
            "turbo" => Some(Self::Turbo),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Lite => "lite",
            Self::Compact => "compact",
            Self::Balanced => "balanced",
            Self::Standard => "standard",
            Self::Turbo => "turbo",
        }
    }

    pub(crate) fn payload_bytes(self) -> usize {
        match self {
            Self::Lite => 3,
            Self::Compact => COMPACT36_BYTES,
            Self::Balanced => 6,
            Self::Standard => 8,
            Self::Turbo => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BaseViewport {
    pub(crate) cols: usize,
    pub(crate) base_rows: usize,
    pub(crate) base_row_px: usize,
    pub(crate) has_row_page: bool,
}

impl Default for BaseViewport {
    fn default() -> Self {
        Self {
            cols: PLANE_COLS,
            base_rows: DEFAULT_ROWS,
            base_row_px: DEFAULT_ROW_PX,
            has_row_page: false,
        }
    }
}

impl BaseViewport {
    pub(crate) fn resolve_live(&self, width: f64, height: f64) -> ViewportConfig {
        let wide_rows = self.cols == WIDE_COLS;
        let (rows, row_px) =
            resolve_live_viewport(self.base_rows, self.base_row_px, width, height, wide_rows);
        ViewportConfig {
            cols: self.cols,
            rows,
            row_px,
            send_bank: wide_rows,
            send_row_page: rows > PAGE_ROWS && self.has_row_page,
        }
    }
}

fn round_to_8(px: f64) -> usize {
    ((px / 8.0).round().max(0.0) as usize) * 8
}

pub(crate) fn resolve_live_viewport(
    base_rows: usize,
    base_row_px: usize,
    width_param: f64,
    height_param: f64,
    wide_rows: bool,
) -> (usize, usize) {
    let b_rows = base_rows.clamp(MIN_ROWS, MAX_ROWS);
    let max_allowed = if wide_rows {
        MAX_ROW_PX
    } else {
        DEFAULT_ROW_PX
    };
    let b_px = round_to_8(base_row_px as f64).clamp(MIN_ROW_PX, max_allowed);
    let row_px_min = round_to_8(b_px as f64 * 0.5).clamp(MIN_ROW_PX, b_px);
    let row_px_max = round_to_8(b_px as f64 * 1.5).clamp(b_px, max_allowed);

    let w = if width_param.is_finite() {
        width_param.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let raw_px = if w < 0.0 {
        b_px as f64 + w * (b_px - row_px_min) as f64
    } else {
        b_px as f64 + w * (row_px_max - b_px) as f64
    };
    let live_row_px = round_to_8(raw_px).clamp(MIN_ROW_PX, max_allowed);

    let h = if height_param.is_finite() {
        height_param.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    let raw_rows = if h < 0.0 {
        b_rows as f64 + h * (b_rows - MIN_ROWS) as f64
    } else {
        b_rows as f64 + h * (MAX_ROWS - b_rows) as f64
    };
    let live_rows = (raw_rows.round() as usize).clamp(MIN_ROWS, MAX_ROWS);

    (live_rows, live_row_px)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ViewportConfig {
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) row_px: usize,
    pub(crate) send_bank: bool,
    pub(crate) send_row_page: bool,
}

impl Default for ViewportConfig {
    fn default() -> Self {
        Self {
            cols: PLANE_COLS,
            rows: DEFAULT_ROWS,
            row_px: DEFAULT_ROW_PX,
            send_bank: false,
            send_row_page: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Packet {
    pub(crate) ctrl: u8,
    pub(crate) data: Vec<u8>,
    pub(crate) bank: u8,
    pub(crate) row_page: u8,
}

impl Packet {
    pub(crate) fn nop(tier: UniTextTier) -> Self {
        Self {
            ctrl: CTRL_NOP,
            data: vec![0; tier.payload_bytes()],
            bank: 0,
            row_page: 0,
        }
    }

    pub(crate) fn clear_all(tier: UniTextTier) -> Self {
        Self {
            ctrl: CTRL_CLEAR_ALL,
            data: vec![0; tier.payload_bytes()],
            bank: 0,
            row_page: 0,
        }
    }

    pub(crate) fn clear_row(tier: UniTextTier, mask: u16) -> Self {
        let mut data = vec![0; tier.payload_bytes()];
        data[0] = (mask & 0x3F) as u8;
        data[1] = ((mask >> 6) & 0x3F) as u8;
        Self {
            ctrl: CTRL_CLEAR_ROW,
            data,
            bank: 0,
            row_page: 0,
        }
    }

    pub(crate) fn set_geometry(tier: UniTextTier, rows: usize, row_px: usize) -> Self {
        let mut data = vec![0; tier.payload_bytes()];
        data[0] = rows.clamp(MIN_ROWS, MAX_ROWS) as u8;
        data[1] = (round_to_8(row_px as f64) / 8).clamp(MIN_ROW_PX8, MAX_ROW_PX8) as u8;
        Self {
            ctrl: CTRL_SET_GEOMETRY,
            data,
            bank: 0,
            row_page: 0,
        }
    }
}

#[derive(Clone)]
pub(crate) struct VirtualScreen {
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    pub(crate) cells: Vec<u16>,
    pub(crate) row_styles: Vec<(u8, u8)>,
    pub(crate) status: u8,
    pub(crate) top_row: u8,
    pub(crate) geometry: Option<(usize, usize)>,
}

impl Default for VirtualScreen {
    fn default() -> Self {
        Self::new(PLANE_COLS, DEFAULT_ROWS)
    }
}

impl VirtualScreen {
    pub(crate) fn new(cols: usize, rows: usize) -> Self {
        assert!(
            cols == PLANE_COLS || cols == WIDE_COLS,
            "cols must be {PLANE_COLS} or {WIDE_COLS}, got {cols}"
        );
        assert!(
            (MIN_ROWS..=MAX_ROWS).contains(&rows),
            "rows must be {MIN_ROWS}..={MAX_ROWS}, got {rows}"
        );
        Self {
            cols,
            rows,
            cells: vec![0; cols * rows],
            row_styles: vec![(0, 0); rows],
            status: 0,
            top_row: 0,
            geometry: None,
        }
    }

    pub(crate) fn visual_to_physical(&self, visual_row: usize) -> usize {
        assert!(
            visual_row < self.rows,
            "visual row {visual_row} out of range 0..{}",
            self.rows
        );
        (visual_row + self.top_row as usize) % self.rows
    }

    pub(crate) fn cell(&self, phys_row: usize, col: usize) -> u16 {
        self.cells[phys_row * self.cols + col]
    }

    #[cfg(test)]
    pub(crate) fn set_cell(&mut self, visual_row: usize, col: usize, cp: u16) {
        assert!(col < self.cols, "col {col} out of range 0..{}", self.cols);
        let phys = self.visual_to_physical(visual_row);
        self.cells[phys * self.cols + col] = cp;
    }

    #[cfg(test)]
    pub(crate) fn visual_row_cells(&self, visual_row: usize) -> Vec<u16> {
        let base = self.visual_to_physical(visual_row) * self.cols;
        self.cells[base..base + self.cols].to_vec()
    }

    #[cfg(test)]
    pub(crate) fn read_visual_cells(&self) -> Vec<u16> {
        let mut out = Vec::with_capacity(self.cols * self.rows);
        for vr in 0..self.rows {
            let base = self.visual_to_physical(vr) * self.cols;
            out.extend_from_slice(&self.cells[base..base + self.cols]);
        }
        out
    }

    #[cfg(test)]
    pub(crate) fn read_visual_row_styles(&self) -> Vec<(u8, u8)> {
        (0..self.rows)
            .map(|vr| self.row_styles[self.visual_to_physical(vr)])
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn visual_row_px(&self, visual_row: usize) -> usize {
        let base = self.visual_to_physical(visual_row) * self.cols;
        self.cells[base..base + self.cols]
            .iter()
            .map(|&cp| {
                if cp == 0 {
                    0
                } else {
                    super::advance::glyph_advance(cp) as usize
                }
            })
            .sum()
    }

    pub(crate) fn clear_all(&mut self) {
        self.cells.fill(0);
        self.row_styles.fill((0, 0));
        self.status = 0;
        self.top_row = 0;
    }

    pub(crate) fn scroll_up(&mut self, lines: usize) {
        if lines == 0 {
            return;
        }
        let old_top = self.top_row as usize;
        for step in 0..lines.min(self.rows) {
            let phys = (old_top + step) % self.rows;
            let base = phys * self.cols;
            self.cells[base..base + self.cols].fill(0);
            self.row_styles[phys] = (0, 0);
        }
        self.top_row = ((old_top + lines) % self.rows) as u8;
    }

    pub(crate) fn set_row_text(&mut self, visual_row: usize, text: &str) {
        let base = self.visual_to_physical(visual_row) * self.cols;
        self.cells[base..base + self.cols].fill(0);
        for (offset, ch) in text.chars().take(self.cols).enumerate() {
            self.cells[base + offset] = normalize_bmp_codepoint(ch);
        }
    }

    pub(crate) fn set_row_style(&mut self, visual_row: usize, color: u8, flags: u8) {
        let phys = self.visual_to_physical(visual_row);
        self.row_styles[phys] = (color & 0x0F, flags & 0x3F);
    }

    pub(crate) fn set_status(&mut self, status: u8) {
        self.status = status & 0x0F;
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.cells.iter().all(|&cp| cp == 0)
            && self.row_styles.iter().all(|&s| s == (0, 0))
            && self.status == 0
            && self.top_row == 0
    }

    pub(crate) fn row_has_content_or_style(&self, phys_row: usize) -> bool {
        if phys_row >= self.rows {
            return false;
        }
        let base = phys_row * self.cols;
        self.cells[base..base + self.cols].iter().any(|&cp| cp != 0)
            || self.row_styles[phys_row] != (0, 0)
    }
}

/// Tracks the expected state of the remote/local UniText 64x12 CRT buffer.
#[derive(Clone)]
pub(crate) struct ShadowCrt {
    rows: usize,
    cells: [u16; MAX_CELLS],
    row_styles: [(u8, u8); MAX_ROWS],
    top_row: u8,
    status: u8,
    geometry: Option<(usize, usize)>,
    known_cells: [bool; MAX_CELLS],
    known_styles: [bool; MAX_ROWS],
    known_top_row: bool,
    known_status: bool,
    known_geometry: bool,
}

impl Default for ShadowCrt {
    fn default() -> Self {
        Self::new(DEFAULT_ROWS)
    }
}

impl ShadowCrt {
    pub(crate) fn new(rows: usize) -> Self {
        assert!(
            (MIN_ROWS..=MAX_ROWS).contains(&rows),
            "rows must be {MIN_ROWS}..={MAX_ROWS}, got {rows}"
        );
        Self {
            rows,
            cells: [0; MAX_CELLS],
            row_styles: [(0, 0); MAX_ROWS],
            top_row: 0,
            status: 0,
            geometry: None,
            known_cells: [true; MAX_CELLS],
            known_styles: [true; MAX_ROWS],
            known_top_row: true,
            known_status: true,
            known_geometry: true,
        }
    }

    /// Clears text/styles/top_row/status while preserving live viewport geometry (CMD_CLEAR_ALL / LocalClear semantics).
    pub(crate) fn reset_cleared(&mut self) {
        let rows = self.rows;
        let geometry = self.geometry;
        let known_geometry = self.known_geometry;
        *self = Self::new(rows);
        self.geometry = geometry;
        self.known_geometry = known_geometry;
    }

    pub(crate) fn cell(&self, phys_row: usize, col: usize) -> u16 {
        self.cells[phys_row * WIDE_COLS + col]
    }

    pub(crate) fn is_cell_known(&self, phys_row: usize, col: usize) -> bool {
        self.known_cells[phys_row * WIDE_COLS + col]
    }

    pub(crate) fn invalidate_all(&mut self) {
        self.known_cells.fill(false);
        self.known_styles.fill(false);
        self.known_top_row = false;
        self.known_status = false;
        self.known_geometry = false;
    }

    pub(crate) fn invalidate_geometry(&mut self) {
        self.known_geometry = false;
    }

    #[cfg(test)]
    pub(crate) fn read_geometry(&self) -> Option<(usize, usize)> {
        self.geometry
    }

    pub(crate) fn invalidate_row(&mut self, phys_row: usize, cols: usize) {
        if phys_row < self.rows {
            let base = phys_row * WIDE_COLS;
            let count = cols.min(WIDE_COLS);
            self.known_cells[base..base + count].fill(false);
            self.known_styles[phys_row] = false;
            self.known_top_row = false;
            self.known_status = false;
        }
    }

    pub(crate) fn invalidate_occupied_row(&mut self, phys_row: usize, target: &VirtualScreen) {
        if phys_row >= self.rows {
            return;
        }
        let has_bank1 = target.cols == WIDE_COLS
            && ((PLANE_COLS..WIDE_COLS).any(|c| target.cell(phys_row, c) != 0)
                || (PLANE_COLS..WIDE_COLS).any(|c| self.cell(phys_row, c) != 0));
        let cols = if has_bank1 { WIDE_COLS } else { PLANE_COLS };
        self.invalidate_row(phys_row, cols);
    }

    pub(crate) fn invalidate_view(&mut self) {
        self.known_top_row = false;
        self.known_status = false;
    }

    fn cell_to_phys_xy(cell: usize, bank: usize, row_page: usize) -> (usize, usize) {
        let phys_row = (cell / PLANE_COLS) + PAGE_ROWS * row_page;
        let col = (cell % PLANE_COLS) + PLANE_COLS * bank;
        (phys_row, col)
    }

    pub(crate) fn apply(&mut self, pkt: &Packet, n_bytes: usize) {
        let mut data = pkt.data.clone();
        data.resize(n_bytes, 0);
        mask_payload(&mut data, n_bytes);
        let bank = (pkt.bank & 1) as usize;
        let row_page = (pkt.row_page & 1) as usize;

        match pkt.ctrl {
            CTRL_NOP => {}
            CTRL_CLEAR_ALL => self.reset_cleared(),
            CTRL_CLEAR_ROW => {
                let mask = (data[0] as u16 & 0x3F) | ((data[1] as u16 & 0x3F) << 6);
                for row in 0..MAX_ROWS {
                    if (mask >> row) & 1 == 1 {
                        let base = row * WIDE_COLS;
                        self.cells[base..base + WIDE_COLS].fill(0);
                        self.known_cells[base..base + WIDE_COLS].fill(true);
                    }
                }
            }
            CTRL_SET_VIEW => {
                let new_top = data[0] & 0x0F;
                let new_status = (data[0] >> 4) & 0x0F;
                let target_row = ((data[1] >> 4) & 0x0F) as usize;
                let color = data[1] & 0x0F;
                let flags = data[2] & 0x3F;
                let upd_status = (data[2] >> 7) & 1;
                if target_row < MAX_ROWS {
                    self.row_styles[target_row] = (color, flags);
                    self.known_styles[target_row] = true;
                }
                if (new_top as usize) < MAX_ROWS {
                    self.top_row = new_top;
                    self.known_top_row = true;
                }
                if upd_status == 1 {
                    self.status = new_status;
                    self.known_status = true;
                }
            }
            CTRL_SET_GEOMETRY => {
                let geo_rows = data[0] as usize;
                let geo_px8 = data[1] as usize;
                if (MIN_ROWS..=MAX_ROWS).contains(&geo_rows)
                    && (MIN_ROW_PX8..=MAX_ROW_PX8).contains(&geo_px8)
                {
                    self.geometry = Some((geo_rows, geo_px8 * 8));
                    self.known_geometry = true;
                }
            }
            1..=96 => {
                let odd = if n_bytes == 3 {
                    data[2] as usize & 1
                } else {
                    0
                };
                let start = (pkt.ctrl as usize - 1) * 2 + odd;
                for i in 0..(n_bytes / 2) {
                    let cell = start + i;
                    if cell < PLANE_CELLS {
                        let (phys_row, col) = Self::cell_to_phys_xy(cell, bank, row_page);
                        let idx = phys_row * WIDE_COLS + col;
                        self.cells[idx] = ((data[2 * i] as u16) << 8) | (data[2 * i + 1] as u16);
                        self.known_cells[idx] = true;
                    }
                }
            }
            97..=144 => {
                let start = (pkt.ctrl as usize - 97) * 4;
                for (i, &lo) in data.iter().take(full_bytes(n_bytes)).enumerate() {
                    let cell = start + i;
                    if cell < PLANE_CELLS {
                        let (phys_row, col) = Self::cell_to_phys_xy(cell, bank, row_page);
                        let idx = phys_row * WIDE_COLS + col;
                        self.cells[idx] = lo as u16;
                        self.known_cells[idx] = true;
                    }
                }
            }
            145..=239 | 243 => {
                let slot = if pkt.ctrl == CTRL_LUT12_LAST {
                    95
                } else {
                    (pkt.ctrl - CTRL_LUT12_MIN) as usize
                };
                if n_bytes == COMPACT36_BYTES {
                    if slot < 66 {
                        let target_row = (slot / 11) + PAGE_ROWS * row_page;
                        let start_col = (slot % 11) * 3;
                        for i in 0..3.min(PLANE_COLS - start_col) {
                            let base = (i / 2) * 3;
                            let lut_idx = if i.is_multiple_of(2) {
                                ((data[base] as u16) << 4) | ((data[base + 1] as u16) >> 4)
                            } else {
                                (((data[base + 1] as u16) & 0x0F) << 8) | (data[base + 2] as u16)
                            };
                            if target_row < MAX_ROWS {
                                let idx = target_row * WIDE_COLS + start_col + i + PLANE_COLS * bank;
                                self.cells[idx] = lut12::lut_idx_to_cp(lut_idx & 0x0FFF);
                                self.known_cells[idx] = true;
                            }
                        }
                    }
                } else {
                    let odd = if n_bytes == 8 {
                        data[7] as usize & 1
                    } else {
                        0
                    };
                    let start = slot * 2 + odd;
                    let count = (n_bytes * 2) / 3;
                    for i in 0..count {
                        let cell = start + i;
                        if cell >= PLANE_CELLS {
                            continue;
                        }
                        let base = (i / 2) * 3;
                        let lut_idx = if i.is_multiple_of(2) {
                            ((data[base] as u16) << 4) | ((data[base + 1] as u16) >> 4)
                        } else {
                            (((data[base + 1] as u16) & 0x0F) << 8) | (data[base + 2] as u16)
                        };
                        let (phys_row, col) = Self::cell_to_phys_xy(cell, bank, row_page);
                        let idx = phys_row * WIDE_COLS + col;
                        self.cells[idx] = lut12::lut_idx_to_cp(lut_idx & 0x0FFF);
                        self.known_cells[idx] = true;
                    }
                }
            }
            244..=249 => {
                let phys_row = (pkt.ctrl - CTRL_PAGE_MIN) as usize + PAGE_ROWS * row_page;
                let start_col = (data[0] & 0x1F) as usize;
                let hi = match (data[0] >> 5) & 0x07 {
                    0 => 0x30u16,
                    1 => 0xFF,
                    2 => 0x04,
                    3 => 0x20,
                    4 => 0x00,
                    _ => return,
                };
                for i in 0..full_bytes(n_bytes).saturating_sub(1) {
                    let col = start_col + i;
                    if col < PLANE_COLS && phys_row < MAX_ROWS {
                        let x = col + PLANE_COLS * bank;
                        let lo = data[1 + i] as u16;
                        let idx = phys_row * WIDE_COLS + x;
                        self.cells[idx] = if lo == 0 { 0 } else { (hi << 8) | lo };
                        self.known_cells[idx] = true;
                    }
                }
            }
            // Reserved Ctrl values (242, 252..=255) -> NOP
            _ => {}
        }
    }

    #[cfg(test)]
    pub(crate) fn load_from_screen(&mut self, screen: &VirtualScreen) {
        assert!(
            screen.rows <= self.rows,
            "screen has {} rows, shadow holds {}",
            screen.rows,
            self.rows
        );
        *self = Self::new(self.rows);
        for phys_row in 0..screen.rows {
            for col in 0..screen.cols {
                self.cells[phys_row * WIDE_COLS + col] = screen.cell(phys_row, col);
            }
            self.row_styles[phys_row] = screen.row_styles[phys_row];
        }
        self.top_row = screen.top_row % (self.rows as u8);
        self.status = screen.status;
        self.geometry = screen.geometry;
    }

    #[cfg(test)]
    pub(crate) fn read_top_row(&self) -> u8 {
        if (self.top_row as usize) < self.rows {
            self.top_row
        } else {
            0
        }
    }

    #[cfg(test)]
    pub(crate) fn read_visual_cells(&self, cols: usize, rows: usize) -> Vec<u16> {
        let top = self.read_top_row() as usize;
        let mut out = Vec::with_capacity(cols * rows);
        for vr in 0..rows {
            let phys = (vr + top) % rows;
            let base = phys * WIDE_COLS;
            out.extend_from_slice(&self.cells[base..base + cols]);
        }
        out
    }

    #[cfg(test)]
    pub(crate) fn read_visual_row_styles(&self) -> Vec<(u8, u8)> {
        let top = self.read_top_row() as usize;
        (0..self.rows)
            .map(|vr| self.row_styles[(vr + top) % self.rows])
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn read_layout(&self, phys_row: usize) -> Vec<(u16, u8)> {
        let base = phys_row * WIDE_COLS;
        super::advance::compute_row_layout(&self.cells[base..base + WIDE_COLS])
    }

    #[cfg(test)]
    pub(crate) fn read_visual_row_px(&self) -> Vec<usize> {
        let top = self.read_top_row() as usize;
        (0..self.rows)
            .map(|vr| {
                let phys = (vr + top) % self.rows;
                let layout = self.read_layout(phys);
                let (x, adv) = layout[WIDE_COLS - 1];
                x as usize + adv as usize
            })
            .collect()
    }

    #[cfg(test)]
    fn visual_row_text(&self, visual_row: usize, cols: usize) -> Vec<u16> {
        let phys = (visual_row + self.read_top_row() as usize) % self.rows;
        self.cells[phys * WIDE_COLS..phys * WIDE_COLS + cols].to_vec()
    }
}

#[derive(Default)]
pub(crate) struct DiffScheduler {
    mismatch_since: [Option<Instant>; MAX_ROWS],
}

impl DiffScheduler {
    pub(crate) fn reset(&mut self) {
        self.mismatch_since = [None; MAX_ROWS];
    }

    pub(crate) fn next_packet(
        &mut self,
        target: &VirtualScreen,
        shadow: &ShadowCrt,
        tier: UniTextTier,
        now: Instant,
    ) -> Option<Packet> {
        let n_bytes = tier.payload_bytes();
        let rows = target.rows;
        let cols = target.cols;
        let status_arg =
            (!shadow.known_status || shadow.status != target.status).then_some(target.status);
        let is_style_dirty =
            |r: usize| !shadow.known_styles[r] || shadow.row_styles[r] != target.row_styles[r];
        let style_for = |phys: usize| {
            let (color, flags) = target.row_styles[phys];
            (phys, color, flags)
        };
        let next_dirty_style = || {
            (0..rows)
                .map(|v| target.visual_to_physical(v))
                .find(|&r| is_style_dirty(r))
                .map(style_for)
        };

        if let Some(geo) = target.geometry
            && (!shadow.known_geometry || shadow.geometry != Some(geo))
        {
            return Some(Packet::set_geometry(tier, geo.0, geo.1));
        }

        // 1. Scroll view changes: clear recycled physical rows BEFORE updating topRow.
        if !shadow.known_top_row || shadow.top_row != target.top_row {
            if shadow.known_top_row && shadow.top_row != target.top_row {
                let shadow_top = (shadow.top_row as usize) % rows;
                let shift = (target.top_row as usize + rows - shadow_top) % rows;
                let mut clear_mask = 0u16;
                for k in 0..shift {
                    let phys = (shadow_top + k) % rows;
                    let has_stale_content = (0..cols).any(|c| {
                        shadow.is_cell_known(phys, c)
                            && shadow.cell(phys, c) != 0
                            && shadow.cell(phys, c) != target.cell(phys, c)
                    });
                    if has_stale_content {
                        clear_mask |= 1u16 << phys;
                    }
                }
                if clear_mask != 0 {
                    return Some(Packet::clear_row(tier, clear_mask));
                }
            }
            return Some(encode_set_view_packet(
                n_bytes,
                Some(target.top_row),
                next_dirty_style(),
                status_arg,
            ));
        }

        // 2. Top-to-bottom visual scheduling within same mismatch age, with anti-mixing clear guard.
        let mut need_write = vec![false; cols * rows];
        let mut dirty_rows = Vec::new();
        let mut empty_clear_mask = 0u16;
        for (phys, mismatch_slot) in self.mismatch_since.iter_mut().enumerate().take(rows) {
            let base = phys * cols;
            let mut row_dirty = false;
            let mut tgt_empty = true;
            for c in 0..cols {
                let tgt_cp = target.cell(phys, c);
                if tgt_cp != 0 {
                    tgt_empty = false;
                }
                if !shadow.is_cell_known(phys, c) || shadow.cell(phys, c) != tgt_cp {
                    need_write[base + c] = true;
                    row_dirty = true;
                }
            }
            if row_dirty {
                if tgt_empty {
                    empty_clear_mask |= 1u16 << phys;
                }
                let since = *mismatch_slot.get_or_insert(now);
                dirty_rows.push((phys, since));
            } else {
                *mismatch_slot = None;
            }
        }

        let top_mod = (target.top_row as usize) % rows;
        if let Some((phys, _)) = dirty_rows.into_iter().min_by_key(|&(phys, since)| {
            let visual = (phys + rows - top_mod) % rows;
            (since, visual)
        }) {
            let base = phys * cols;
            let tgt_empty = (0..cols).all(|c| target.cell(phys, c) == 0);
            if tgt_empty {
                return Some(Packet::clear_row(tier, empty_clear_mask));
            }

            let conflicts = (0..cols)
                .filter(|&c| {
                    shadow.is_cell_known(phys, c)
                        && shadow.cell(phys, c) != 0
                        && shadow.cell(phys, c) != target.cell(phys, c)
                })
                .count();
            if conflicts > ((n_bytes * 2) / 3).max(1) {
                return Some(Packet::clear_row(tier, 1u16 << phys));
            }

            if is_style_dirty(phys) {
                return Some(encode_set_view_packet(
                    n_bytes,
                    Some(target.top_row),
                    Some(style_for(phys)),
                    status_arg,
                ));
            }

            if let Some(first_col) = (0..cols).find(|&c| need_write[base + c]) {
                let row_page = (phys / PAGE_ROWS) as u8;
                let page_row = phys % PAGE_ROWS;
                let bank = (first_col / PLANE_COLS) as u8;
                let bank_col = first_col % PLANE_COLS;
                let plane_cell = page_row * PLANE_COLS + bank_col;

                let page_start_row = (row_page as usize) * PAGE_ROWS;
                let page_rows = PAGE_ROWS.min(rows - page_start_row);
                let mut plane_cells = [0u16; PLANE_CELLS];
                let mut plane_need = [false; PLANE_CELLS];
                for pr in 0..page_rows {
                    let r = page_start_row + pr;
                    for bc in 0..PLANE_COLS {
                        let col = (bank as usize) * PLANE_COLS + bc;
                        let dst = pr * PLANE_COLS + bc;
                        plane_cells[dst] = target.cell(r, col);
                        plane_need[dst] = need_write[r * cols + col];
                    }
                }

                if let Some(pkt) = encode_first_plane_packet(
                    &plane_cells,
                    &plane_need,
                    plane_cell,
                    n_bytes,
                    bank,
                    row_page,
                ) {
                    return Some(pkt);
                }
            }
        }

        // 3. Remaining row styles and status updates.
        if let Some(style) = next_dirty_style() {
            return Some(encode_set_view_packet(
                n_bytes,
                Some(target.top_row),
                Some(style),
                status_arg,
            ));
        }

        status_arg
            .map(|status| encode_set_view_packet(n_bytes, Some(target.top_row), None, Some(status)))
    }
}

pub(crate) fn normalize_bmp_codepoint(ch: char) -> u16 {
    let code = ch as u32;
    if code > 0xFFFF { 0xFFFD } else { code as u16 }
}

fn page_selector(page_hi: u8) -> Option<u8> {
    match page_hi {
        0x30 => Some(0),
        0xFF => Some(1),
        0x04 => Some(2),
        0x20 => Some(3),
        0x00 => Some(4),
        _ => None,
    }
}

fn encode_set_view_packet(
    n_bytes: usize,
    top_row: Option<u8>,
    style: Option<(usize, u8, u8)>,
    status: Option<u8>,
) -> Packet {
    let upd_status = u8::from(status.is_some());
    let status_val = status.unwrap_or(0) & 0x0F;
    let top_val = top_row.map(|t| t & 0x0F).unwrap_or(0x0F);
    let d0 = (status_val << 4) | top_val;
    let (d1, d2) = match style {
        Some((row, color, flags)) => (
            (((row as u8) & 0x0F) << 4) | (color & 0x0F),
            (upd_status << 7) | (flags & 0x3F),
        ),
        None => (0xF0, upd_status << 7),
    };
    let mut data = vec![0; n_bytes];
    data[0] = d0;
    data[1] = d1;
    data[2] = d2;
    Packet {
        ctrl: CTRL_SET_VIEW,
        data,
        bank: 0,
        row_page: 0,
    }
}

fn encode_first_plane_packet(
    plane_cells: &[u16; PLANE_CELLS],
    need_write: &[bool; PLANE_CELLS],
    cell: usize,
    n_bytes: usize,
    bank: u8,
    row_page: u8,
) -> Option<Packet> {
    if !need_write[cell] {
        return None;
    }

    let lut_count = (n_bytes * 2) / 3;
    let ascii_count = full_bytes(n_bytes);
    let page_count = ascii_count.saturating_sub(1);
    let utf16_count = (n_bytes / 2).max(1);

    let mut best: Option<(usize, u8, Packet)> = None;
    let mut consider = |score: usize, prio: u8, pkt: Packet| {
        if best
            .as_ref()
            .is_none_or(|(b_score, b_prio, _)| (score, prio) > (*b_score, *b_prio))
        {
            best = Some((score, prio, pkt));
        }
    };

    // Candidate A: WRITE_ASCII
    let ascii_start = (cell / 4) * 4;
    let ascii_end = PLANE_CELLS.min(ascii_start + ascii_count);
    if (ascii_start..ascii_end).contains(&cell)
        && plane_cells[ascii_start..ascii_end]
            .iter()
            .all(|&cp| cp <= 0xFF)
    {
        let score = (ascii_start..ascii_end).filter(|&k| need_write[k]).count();
        let mut data = vec![0; n_bytes];
        for (dst, &cp) in data.iter_mut().zip(&plane_cells[ascii_start..ascii_end]) {
            *dst = cp as u8;
        }
        consider(
            score,
            4,
            Packet {
                ctrl: CTRL_ASCII_MIN + (ascii_start / 4) as u8,
                data,
                bank,
                row_page,
            },
        );
    }

    // Candidate B: WRITE_PAGE (clamped to current page_row within the (bank, row_page) plane)
    if page_count > 0 {
        let page_row = cell / PLANE_COLS;
        let col = cell % PLANE_COLS;
        let row_base = page_row * PLANE_COLS;
        let page_hi = (plane_cells[cell] >> 8) as u8;
        if let Some(page_sel) = page_selector(page_hi) {
            let end_col = PLANE_COLS.min(col + page_count);
            let mut valid = true;
            let mut data = vec![0; n_bytes];
            data[0] = (page_sel << 5) | (col as u8 & 0x1F);
            for (i, c_col) in (col..end_col).enumerate() {
                let cp = plane_cells[row_base + c_col];
                if cp == 0 {
                    data[1 + i] = 0;
                } else if (cp >> 8) as u8 == page_hi && (cp & 0xFF) != 0 {
                    data[1 + i] = (cp & 0xFF) as u8;
                } else {
                    valid = false;
                    break;
                }
            }
            if valid {
                let score = ((row_base + col)..(row_base + end_col))
                    .filter(|&k| need_write[k])
                    .count();
                consider(
                    score,
                    3,
                    Packet {
                        ctrl: CTRL_PAGE_MIN + page_row as u8,
                        data,
                        bank,
                        row_page,
                    },
                );
            }
        }
    }

    // Candidate C: WRITE_LUT12
    if lut_count > 0 {
        if n_bytes == COMPACT36_BYTES {
            let page_row = cell / PLANE_COLS;
            let chunk = (cell % PLANE_COLS) / 3;
            let start_col = chunk * 3;
            let end_col = PLANE_COLS.min(start_col + 3);
            let l_start = page_row * PLANE_COLS + start_col;
            let l_end = page_row * PLANE_COLS + end_col;
            let mut indices = [0u16; 3];
            let mut all_mapped = true;
            for (offset, idx) in (l_start..l_end).enumerate() {
                if let Some(lut_idx) = lut12::cp_to_lut_idx(plane_cells[idx]) {
                    indices[offset] = lut_idx;
                } else {
                    all_mapped = false;
                    break;
                }
            }
            if all_mapped {
                let [a, b, c] = indices;
                let data = vec![
                    (a >> 4) as u8,
                    (((a & 0x0F) << 4) | ((b >> 8) & 0x0F)) as u8,
                    (b & 0xFF) as u8,
                    (c >> 4) as u8,
                    ((c & 0x0F) << 4) as u8,
                ];
                let slot = (page_row * 11 + chunk) as u8;
                let score = (l_start..l_end).filter(|&k| need_write[k]).count();
                consider(
                    score,
                    1,
                    Packet {
                        ctrl: CTRL_LUT12_MIN + slot,
                        data,
                        bank,
                        row_page,
                    },
                );
            }
        } else {
            let mut starts = [cell - (cell % 2), usize::MAX];
            if n_bytes == 8 && (cell % 2) == 1 {
                starts[1] = cell;
            }
            for l_start in starts {
                if l_start == usize::MAX {
                    continue;
                }
                let l_end = PLANE_CELLS.min(l_start + lut_count);
                if !(l_start..l_end).contains(&cell) {
                    continue;
                }
                let mut indices = Vec::with_capacity(lut_count);
                let mut all_mapped = true;
                for i in 0..lut_count {
                    let idx = l_start + i;
                    let cp = plane_cells.get(idx).copied().unwrap_or(0);
                    if let Some(lut_idx) = lut12::cp_to_lut_idx(cp) {
                        indices.push(lut_idx);
                    } else {
                        all_mapped = false;
                        break;
                    }
                }
                if all_mapped {
                    let mut data = vec![0; n_bytes];
                    for p_i in (0..indices.len()).step_by(2) {
                        let a = indices[p_i];
                        let b_off = (p_i / 2) * 3;
                        if p_i + 1 < indices.len() {
                            let b = indices[p_i + 1];
                            data[b_off] = (a >> 4) as u8;
                            data[b_off + 1] = (((a & 0x0F) << 4) | ((b >> 8) & 0x0F)) as u8;
                            data[b_off + 2] = (b & 0xFF) as u8;
                        } else {
                            data[b_off] = (a >> 4) as u8;
                            data[b_off + 1] =
                                (((a & 0x0F) << 4) | if n_bytes == 8 { l_start as u16 & 1 } else { 0 })
                                    as u8;
                        }
                    }
                    let slot = (l_start / 2) as u8;
                    let ctrl = if slot == 95 {
                        CTRL_LUT12_LAST
                    } else {
                        CTRL_LUT12_MIN + slot
                    };
                    let score = (l_start..l_end).filter(|&k| need_write[k]).count();
                    consider(
                        score,
                        1,
                        Packet {
                            ctrl,
                            data,
                            bank,
                            row_page,
                        },
                    );
                }
            }
        }
    }

    // Candidate D: WRITE_UTF16
    if n_bytes == 3 {
        let cp = plane_cells[cell];
        let data = vec![(cp >> 8) as u8, (cp & 0xFF) as u8, (cell & 1) as u8];
        consider(
            1,
            2,
            Packet {
                ctrl: CTRL_UTF16_MIN + (cell / 2) as u8,
                data,
                bank,
                row_page,
            },
        );
    } else {
        let u_start = cell - (cell % 2);
        let u_end = PLANE_CELLS.min(u_start + utf16_count);
        let mut data = vec![0; n_bytes];
        for i in 0..utf16_count {
            let cp = plane_cells.get(u_start + i).copied().unwrap_or(0);
            data[2 * i] = (cp >> 8) as u8;
            data[2 * i + 1] = (cp & 0xFF) as u8;
        }
        let score = (u_start..u_end).filter(|&k| need_write[k]).count();
        consider(
            score,
            2,
            Packet {
                ctrl: CTRL_UTF16_MIN + (u_start / 2) as u8,
                data,
                bank,
                row_page,
            },
        );
    }

    best.map(|(_, _, pkt)| pkt)
}

pub(crate) fn encode_osc_bundle(
    packet: &Packet,
    n_bytes: usize,
    send_bank: bool,
    send_row_page: bool,
) -> Result<Vec<u8>, String> {
    let int_bytes = full_bytes(n_bytes);
    let nibble_bits = if n_bytes == COMPACT36_BYTES { 4 } else { 0 };
    let extra = nibble_bits + usize::from(send_bank) + usize::from(send_row_page) + 1;
    let mut content = Vec::with_capacity(int_bytes + extra);
    for idx in 0..int_bytes {
        let val = packet.data.get(idx).copied().unwrap_or(0) as i32;
        content.push(OscPacket::Message(OscMessage {
            addr: format!("/avatar/parameters/UniText_D{idx}"),
            args: vec![OscType::Int(val)],
        }));
    }
    if n_bytes == COMPACT36_BYTES {
        let d4 = packet.data.get(4).copied().unwrap_or(0);
        for bit in 4..8 {
            content.push(OscPacket::Message(OscMessage {
                addr: format!("/avatar/parameters/UniText_D4b{bit}"),
                args: vec![OscType::Bool(((d4 >> bit) & 1) != 0)],
            }));
        }
    }
    if send_bank {
        content.push(OscPacket::Message(OscMessage {
            addr: "/avatar/parameters/UniText_Bank".into(),
            args: vec![OscType::Bool(packet.bank != 0)],
        }));
    }
    if send_row_page {
        content.push(OscPacket::Message(OscMessage {
            addr: "/avatar/parameters/UniText_RowPage".into(),
            args: vec![OscType::Bool(packet.row_page != 0)],
        }));
    }
    content.push(OscPacket::Message(OscMessage {
        addr: "/avatar/parameters/UniText_Ctrl".into(),
        args: vec![OscType::Int(packet.ctrl as i32)],
    }));
    encoder::encode(&OscPacket::Bundle(OscBundle {
        timetag: OscTime {
            seconds: 0,
            fractional: 1,
        },
        content,
    }))
    .map_err(|error| error.to_string())
}

pub(crate) fn encode_visible_packet(visible: bool) -> Result<Vec<u8>, String> {
    encoder::encode(&OscPacket::Message(OscMessage {
        addr: "/avatar/parameters/UniText_Visible".into(),
        args: vec![OscType::Bool(visible)],
    }))
    .map_err(|error| error.to_string())
}

pub(crate) const OSC_PARAM_WIDTH: &str = "/avatar/parameters/UniText_Width";
pub(crate) const OSC_PARAM_HEIGHT: &str = "/avatar/parameters/UniText_Height";
pub(crate) const OSC_PARAM_LOCAL_CLEAR: &str = "/avatar/parameters/UniText_LocalClear";

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum LiveOscParamEvent {
    Width(f64),
    Height(f64),
    LocalClear(bool),
}

pub(crate) fn decode_live_osc_events(bytes: &[u8]) -> Vec<LiveOscParamEvent> {
    let Ok((_, packet)) = rosc::decoder::decode_udp(bytes) else {
        return Vec::new();
    };
    let mut events = Vec::new();
    collect_live_osc_packet(&packet, &mut events);
    events
}

fn collect_live_osc_packet(packet: &OscPacket, out: &mut Vec<LiveOscParamEvent>) {
    match packet {
        OscPacket::Message(msg) => {
            let Some(arg) = msg.args.first() else {
                return;
            };
            match msg.addr.as_str() {
                OSC_PARAM_WIDTH => {
                    if let Some(v) = osc_arg_axis(arg) {
                        out.push(LiveOscParamEvent::Width(v));
                    }
                }
                OSC_PARAM_HEIGHT => {
                    if let Some(v) = osc_arg_axis(arg) {
                        out.push(LiveOscParamEvent::Height(v));
                    }
                }
                OSC_PARAM_LOCAL_CLEAR => {
                    if let Some(v) = osc_arg_bool(arg) {
                        out.push(LiveOscParamEvent::LocalClear(v));
                    }
                }
                _ => {}
            }
        }
        OscPacket::Bundle(bundle) => {
            for inner in &bundle.content {
                collect_live_osc_packet(inner, out);
            }
        }
    }
}

fn osc_arg_axis(arg: &OscType) -> Option<f64> {
    let val = match arg {
        OscType::Float(v) => f64::from(*v),
        OscType::Double(v) => *v,
        OscType::Int(v) => f64::from(*v),
        _ => return None,
    };
    val.is_finite().then_some(val.clamp(-1.0, 1.0))
}

fn osc_arg_bool(arg: &OscType) -> Option<bool> {
    match arg {
        OscType::Bool(v) => Some(*v),
        OscType::Int(v) => Some(*v != 0),
        OscType::Float(v) => Some(*v != 0.0),
        _ => None,
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct OscQueryAvatarInfo {
    pub(crate) tier: Option<UniTextTier>,
    pub(crate) proto_ver: Option<u32>,
    pub(crate) base_viewport: BaseViewport,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) viewport: ViewportConfig,
    pub(crate) has_unitext_params: bool,
    pub(crate) has_resize_axes: bool,
    pub(crate) avatar_id: Option<String>,
    pub(crate) local_clear: bool,
}

#[derive(Default)]
struct OscQueryCollector {
    names: HashSet<String>,
    proto_ver: Option<u32>,
    raw_rows: Option<i64>,
    raw_row_px8: Option<i64>,
    raw_width: Option<f64>,
    raw_height: Option<f64>,
    avatar_id: Option<String>,
    local_clear: bool,
}

pub(crate) fn parse_oscquery_avatar(tree: &Value) -> OscQueryAvatarInfo {
    let mut acc = OscQueryCollector::default();
    collect_avatar_info(tree, None, &mut acc);
    let tier = if acc.names.contains("UniText_D15") {
        Some(UniTextTier::Turbo)
    } else if acc.names.contains("UniText_D7") {
        Some(UniTextTier::Standard)
    } else if acc.names.contains("UniText_D5") {
        Some(UniTextTier::Balanced)
    } else if acc.names.contains("UniText_D3") {
        Some(UniTextTier::Compact)
    } else if acc.names.contains("UniText_D2") {
        Some(UniTextTier::Lite)
    } else {
        None
    };

    let has_bank = acc.names.contains("UniText_Bank");
    let has_row_page = acc.names.contains("UniText_RowPage");
    let cols = if has_bank { WIDE_COLS } else { PLANE_COLS };
    let max_allowed_px = if has_bank { MAX_ROW_PX } else { DEFAULT_ROW_PX };
    let base_rows = acc
        .raw_rows
        .map(|v| v.clamp(MIN_ROWS as i64, MAX_ROWS as i64) as usize)
        .unwrap_or(DEFAULT_ROWS);
    let base_row_px = acc
        .raw_row_px8
        .map(|v| {
            v.saturating_mul(8)
                .clamp(MIN_ROW_PX as i64, max_allowed_px as i64) as usize
        })
        .unwrap_or(DEFAULT_ROW_PX);
    let width = acc.raw_width.map(|v| v.clamp(-1.0, 1.0)).unwrap_or(0.0);
    let height = acc.raw_height.map(|v| v.clamp(-1.0, 1.0)).unwrap_or(0.0);
    let base_viewport = BaseViewport {
        cols,
        base_rows,
        base_row_px,
        has_row_page,
    };
    let viewport = base_viewport.resolve_live(width, height);

    let has_resize_axes = acc.names.contains("UniText_Width") || acc.names.contains("UniText_Height");

    OscQueryAvatarInfo {
        tier,
        proto_ver: acc.proto_ver,
        base_viewport,
        width,
        height,
        viewport,
        has_unitext_params: !acc.names.is_empty(),
        has_resize_axes,
        avatar_id: acc.avatar_id,
        local_clear: acc.local_clear,
    }
}

fn first_oscquery_value(node: &serde_json::Map<String, Value>) -> Option<&Value> {
    let value = node.get("VALUE")?;
    value.as_array().and_then(|arr| arr.first()).or(Some(value))
}

fn first_oscquery_int(node: &serde_json::Map<String, Value>) -> Option<i64> {
    let val = first_oscquery_value(node)?;
    if let Some(i) = val.as_i64() {
        Some(i)
    } else if let Some(u) = val.as_u64() {
        i64::try_from(u).ok()
    } else {
        val.as_f64()
            .filter(|f| f.is_finite())
            .map(|f| f.round() as i64)
    }
}

fn first_oscquery_float(node: &serde_json::Map<String, Value>) -> Option<f64> {
    first_oscquery_value(node)?
        .as_f64()
        .filter(|f| f.is_finite())
}

fn collect_avatar_info(node: &Value, key_name: Option<&str>, acc: &mut OscQueryCollector) {
    let Some(obj) = node.as_object() else {
        return;
    };
    let full_path = obj.get("FULL_PATH").and_then(Value::as_str);
    let leaf = full_path
        .and_then(|path| path.trim_end_matches('/').rsplit('/').next())
        .or(key_name);

    if let Some(name) = leaf
        && name.starts_with("UniText_")
    {
        acc.names.insert(name.to_owned());
        match name {
            "UniText_LocalClear" => {
                if let Some(val) = first_oscquery_value(obj).and_then(Value::as_bool) {
                    acc.local_clear = val;
                }
            }
            "UniText_ProtoVer" => {
                if let Some(v) = first_oscquery_int(obj).and_then(|v| u32::try_from(v).ok()) {
                    acc.proto_ver = Some(v);
                }
            }
            "UniText_Rows" => {
                if let Some(v) = first_oscquery_int(obj) {
                    acc.raw_rows = Some(v);
                }
            }
            "UniText_RowPx8" => {
                if let Some(v) = first_oscquery_int(obj) {
                    acc.raw_row_px8 = Some(v);
                }
            }
            "UniText_Width" => {
                if let Some(v) = first_oscquery_float(obj) {
                    acc.raw_width = Some(v);
                }
            }
            "UniText_Height" => {
                if let Some(v) = first_oscquery_float(obj) {
                    acc.raw_height = Some(v);
                }
            }
            _ => {}
        }
    }
    if full_path == Some("/avatar/change")
        && let Some(id) = first_oscquery_value(obj)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
    {
        acc.avatar_id = Some(id.to_owned());
    }

    if let Some(contents) = obj.get("CONTENTS").and_then(Value::as_object) {
        for (child_key, child_val) in contents {
            collect_avatar_info(child_val, Some(child_key.as_str()), acc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use std::collections::HashMap;

    fn deserialize_present_option<'de, D, T>(de: D) -> Result<Option<Option<T>>, D::Error>
    where
        D: serde::Deserializer<'de>,
        T: serde::Deserialize<'de>,
    {
        Ok(Some(Option::<T>::deserialize(de)?))
    }

    #[derive(Deserialize)]
    struct TestVectorsDoc {
        protocol_version: u32,
        vectors: Vec<TestVector>,
    }

    #[derive(Deserialize)]
    struct TestVector {
        id: String,
        tier: String,
        payload_bytes: usize,
        row_cells: usize,
        rows: usize,
        row_px: usize,
        #[serde(default)]
        encoder_check: bool,
        max_encoder_packets: Option<usize>,
        #[serde(default)]
        scroll_up: usize,
        initial_state: VectorState,
        packets: Vec<VectorPacket>,
        expected_state: VectorState,
        expected_row_px: Option<Vec<usize>>,
        #[serde(default)]
        expected_layout: HashMap<String, Vec<u16>>,
        #[serde(default, deserialize_with = "deserialize_present_option")]
        expected_geometry: Option<Option<[usize; 2]>>,
    }

    #[derive(Deserialize, Default)]
    struct VectorState {
        #[serde(default)]
        top_row: u8,
        #[serde(default)]
        rows: HashMap<String, String>,
        #[serde(default)]
        sparse_cells: Vec<SparseCell>,
        #[serde(default)]
        row_styles: Vec<VectorRowStyle>,
        #[serde(default)]
        status: u8,
    }

    #[derive(Deserialize)]
    struct SparseCell {
        row: usize,
        col: usize,
        cp: u16,
    }

    #[derive(Deserialize)]
    struct VectorRowStyle {
        row: usize,
        color: u8,
        flags: u8,
    }

    #[derive(Deserialize)]
    struct VectorPacket {
        ctrl: u8,
        data: Vec<u8>,
        #[serde(default)]
        bank: u8,
        #[serde(default)]
        row_page: u8,
    }

    fn build_screen_from_state(state: &VectorState, cols: usize, rows: usize) -> VirtualScreen {
        let mut screen = VirtualScreen::new(cols, rows);
        screen.top_row = state.top_row % (rows as u8);
        for (row_str, text) in &state.rows {
            let row: usize = row_str.parse().expect("valid row key");
            screen.set_row_text(row, text);
        }
        for sparse in &state.sparse_cells {
            screen.set_cell(sparse.row, sparse.col, sparse.cp);
        }
        for style in &state.row_styles {
            screen.set_row_style(style.row, style.color, style.flags);
        }
        screen.set_status(state.status);
        screen
    }

    fn apply_idempotent_15_frames(sim: &mut ShadowCrt, pkt: &Packet, n_bytes: usize, vid: &str) {
        let mut snapshot_cells = None;
        let mut snapshot_styles = None;
        let mut snapshot_top = None;
        let mut snapshot_status = None;
        let mut snapshot_geo = None;
        for frame in 0..15 {
            sim.apply(pkt, n_bytes);
            if frame == 0 {
                snapshot_cells = Some(sim.cells);
                snapshot_styles = Some(sim.row_styles);
                snapshot_top = Some(sim.top_row);
                snapshot_status = Some(sim.status);
                snapshot_geo = Some(sim.geometry);
            } else {
                assert_eq!(
                    Some(sim.cells),
                    snapshot_cells,
                    "{vid}: non-idempotent cells on ctrl={}",
                    pkt.ctrl
                );
                assert_eq!(
                    Some(sim.row_styles),
                    snapshot_styles,
                    "{vid}: non-idempotent styles on ctrl={}",
                    pkt.ctrl
                );
                assert_eq!(
                    Some(sim.top_row),
                    snapshot_top,
                    "{vid}: non-idempotent top_row on ctrl={}",
                    pkt.ctrl
                );
                assert_eq!(
                    Some(sim.status),
                    snapshot_status,
                    "{vid}: non-idempotent status on ctrl={}",
                    pkt.ctrl
                );
                assert_eq!(
                    Some(sim.geometry),
                    snapshot_geo,
                    "{vid}: non-idempotent geometry on ctrl={}",
                    pkt.ctrl
                );
            }
        }
    }

    #[test]
    fn passes_all_unitext_v2_test_vectors() {
        let raw = include_str!("../../tests/fixtures/unitext/test-vectors.json");
        let doc: TestVectorsDoc = serde_json::from_str(raw).expect("test-vectors.json must parse");
        assert_eq!(doc.protocol_version, 2);
        assert_eq!(doc.vectors.len(), 11);

        for vec in &doc.vectors {
            let tier = UniTextTier::from_str_name(&vec.tier)
                .unwrap_or_else(|| panic!("{}: unknown tier {}", vec.id, vec.tier));
            let n_bytes = tier.payload_bytes();
            assert_eq!(vec.payload_bytes, n_bytes, "{}: payload_bytes", vec.id);

            let init_screen = build_screen_from_state(&vec.initial_state, vec.row_cells, vec.rows);
            let expected_screen =
                build_screen_from_state(&vec.expected_state, vec.row_cells, vec.rows);

            let mut sim = ShadowCrt::new(vec.rows);
            sim.load_from_screen(&init_screen);
            for raw_pkt in &vec.packets {
                assert_eq!(raw_pkt.data.len(), n_bytes, "{}: pkt data len", vec.id);
                let pkt = Packet {
                    ctrl: raw_pkt.ctrl,
                    data: raw_pkt.data.clone(),
                    bank: raw_pkt.bank,
                    row_page: raw_pkt.row_page,
                };
                apply_idempotent_15_frames(&mut sim, &pkt, n_bytes, &vec.id);
            }

            assert_eq!(
                sim.read_top_row(),
                expected_screen.top_row,
                "{}: top_row mismatch",
                vec.id
            );
            assert_eq!(
                sim.read_visual_cells(vec.row_cells, vec.rows),
                expected_screen.read_visual_cells(),
                "{}: visual cells mismatch",
                vec.id
            );
            if vec.row_cells < WIDE_COLS {
                let wide_cells = sim.read_visual_cells(WIDE_COLS, vec.rows);
                for r in 0..vec.rows {
                    for c in vec.row_cells..WIDE_COLS {
                        assert_eq!(
                            wide_cells[r * WIDE_COLS + c],
                            0,
                            "{}: narrow vector wrote beyond col {}",
                            vec.id,
                            vec.row_cells
                        );
                    }
                }
            }
            assert_eq!(
                sim.read_visual_row_styles(),
                expected_screen.read_visual_row_styles(),
                "{}: row styles mismatch",
                vec.id
            );
            assert_eq!(
                sim.status, expected_screen.status,
                "{}: status mismatch",
                vec.id
            );
            if let Some(exp_geo) = vec.expected_geometry {
                assert_eq!(
                    sim.read_geometry(),
                    exp_geo.map(|[r, px]| (r, px)),
                    "{}: geometry mismatch",
                    vec.id
                );
            }

            let got_px = sim.read_visual_row_px();
            let exp_px: Vec<usize> = (0..vec.rows)
                .map(|r| expected_screen.visual_row_px(r))
                .collect();
            assert_eq!(got_px, exp_px, "{}: row px mismatch", vec.id);
            if let Some(ref spec_px) = vec.expected_row_px {
                assert_eq!(&got_px, spec_px, "{}: expected_row_px mismatch", vec.id);
            }
            assert!(
                got_px.iter().all(|&px| px <= vec.row_px),
                "{}: visual row px {got_px:?} exceeds {}",
                vec.id,
                vec.row_px
            );

            for (vr_str, exp_x) in &vec.expected_layout {
                let vr: usize = vr_str.parse().expect("layout row index");
                let phys = (vr + sim.read_top_row() as usize) % vec.rows;
                let got_x: Vec<u16> = sim
                    .read_layout(phys)
                    .into_iter()
                    .take(exp_x.len())
                    .map(|(x, _)| x)
                    .collect();
                assert_eq!(&got_x, exp_x, "{}: layout row {vr} mismatch", vec.id);
            }

            if vec.encoder_check {
                let mut working = init_screen.clone();
                if vec.scroll_up > 0 {
                    working.scroll_up(vec.scroll_up);
                }
                let exp_visual_cells = expected_screen.read_visual_cells();
                let exp_visual_styles = expected_screen.read_visual_row_styles();
                for r in 0..vec.rows {
                    for c in 0..vec.row_cells {
                        working.set_cell(r, c, exp_visual_cells[r * vec.row_cells + c]);
                    }
                    let (color, flags) = exp_visual_styles[r];
                    working.set_row_style(r, color, flags);
                }
                working.set_status(expected_screen.status);

                let mut enc_sim = ShadowCrt::new(vec.rows);
                enc_sim.load_from_screen(&init_screen);
                let mut sched = DiffScheduler::default();
                let now = Instant::now();
                let mut emitted = Vec::new();
                while let Some(pkt) = sched.next_packet(&working, &enc_sim, tier, now) {
                    if vec.row_cells == PLANE_COLS {
                        assert_eq!(pkt.bank, 0, "{}: narrow mode emitted bank=1", vec.id);
                    } else {
                        assert!(pkt.bank <= 1, "{}: invalid bank", vec.id);
                    }
                    if vec.rows <= PAGE_ROWS {
                        assert_eq!(pkt.row_page, 0, "{}: <=6 rows emitted row_page=1", vec.id);
                    } else {
                        assert!(pkt.row_page <= 1, "{}: invalid row_page", vec.id);
                    }
                    apply_idempotent_15_frames(&mut enc_sim, &pkt, n_bytes, &vec.id);
                    emitted.push(pkt);
                    assert!(
                        emitted.len() <= 200,
                        "{}: encoder loop exceeded safety limit",
                        vec.id
                    );
                }

                if let Some(max_pkts) = vec.max_encoder_packets {
                    assert!(
                        emitted.len() <= max_pkts,
                        "{}: encoder emitted {} packets, exceeding max_encoder_packets={max_pkts}",
                        vec.id,
                        emitted.len()
                    );
                }

                assert_eq!(
                    enc_sim.read_top_row(),
                    expected_screen.top_row,
                    "{}: encoder top_row mismatch",
                    vec.id
                );
                assert_eq!(
                    enc_sim.read_visual_cells(vec.row_cells, vec.rows),
                    exp_visual_cells,
                    "{}: encoder visual cells mismatch",
                    vec.id
                );
                assert_eq!(
                    enc_sim.read_visual_row_styles(),
                    exp_visual_styles,
                    "{}: encoder styles mismatch",
                    vec.id
                );
                assert_eq!(
                    enc_sim.status, expected_screen.status,
                    "{}: encoder status mismatch",
                    vec.id
                );
            }
        }
    }

    #[test]
    fn v1_diff_scheduler_multilingual_and_ring_scroll() {
        let mut target = VirtualScreen::new(WIDE_COLS, 8);
        target.set_row_text(0, "Hello VRCS!");
        target.set_row_text(1, "中文字幕测试");
        target.set_row_text(2, "おはようございます");
        target.set_row_text(3, "안녕하세요");
        target.set_row_text(7, "Page 1 bottom line");
        target.set_row_style(0, 1, 1);
        target.set_row_style(1, 3, 2);
        target.set_row_style(7, 4, 2);
        target.set_status(2);

        let mut shadow = ShadowCrt::new(8);
        let mut sched = DiffScheduler::default();
        let mut now = Instant::now();
        let tier = UniTextTier::Standard;
        for _ in 0..64 {
            let Some(pkt) = sched.next_packet(&target, &shadow, tier, now) else {
                break;
            };
            shadow.apply(&pkt, tier.payload_bytes());
            now += std::time::Duration::from_millis(150);
        }
        for r in 0..8 {
            assert_eq!(
                shadow.visual_row_text(r, WIDE_COLS),
                target.visual_row_cells(r)
            );
        }
        assert_eq!(shadow.status, 2);

        target.scroll_up(1);
        target.set_row_text(7, "滚动新行 OK");
        target.set_row_style(7, 2, 2);
        let first = sched
            .next_packet(&target, &shadow, UniTextTier::Lite, now)
            .unwrap();
        assert_eq!(first.ctrl, CTRL_CLEAR_ROW);
        shadow.apply(&first, UniTextTier::Lite.payload_bytes());
        let second = sched
            .next_packet(&target, &shadow, UniTextTier::Lite, now)
            .unwrap();
        assert_eq!(second.ctrl, CTRL_SET_VIEW);
        shadow.apply(&second, UniTextTier::Lite.payload_bytes());

        for _ in 0..32 {
            let Some(pkt) = sched.next_packet(&target, &shadow, UniTextTier::Lite, now) else {
                break;
            };
            shadow.apply(&pkt, UniTextTier::Lite.payload_bytes());
        }
        for r in 0..8 {
            assert_eq!(
                shadow.visual_row_text(r, WIDE_COLS),
                target.visual_row_cells(r)
            );
        }
    }

    #[test]
    fn diff_scheduler_avoids_row_mixing_and_refresh_heals_ghosts() {
        let mut target = VirtualScreen::default();
        target.set_row_text(0, "First sentence is very long!");
        let mut shadow = ShadowCrt::default();
        let mut sched = DiffScheduler::default();
        let now = Instant::now();
        let tier = UniTextTier::Compact;
        while let Some(pkt) = sched.next_packet(&target, &shadow, tier, now) {
            shadow.apply(&pkt, tier.payload_bytes());
        }

        // Completely replacing a long line on a narrow tier emits CLEAR_ROW before writing.
        target.set_row_text(0, "Brand new short text");
        let pkt = sched.next_packet(&target, &shadow, tier, now).unwrap();
        assert_eq!(pkt.ctrl, CTRL_CLEAR_ROW);
        shadow.apply(&pkt, tier.payload_bytes());
        while let Some(pkt) = sched.next_packet(&target, &shadow, tier, now) {
            shadow.apply(&pkt, tier.payload_bytes());
        }
        assert_eq!(
            shadow.visual_row_text(0, PLANE_COLS),
            target.visual_row_cells(0)
        );

        // Simulate remote ghost character in trailing empty cell + self-healing refresh.
        shadow.cells[30] = 'X' as u16;
        shadow.invalidate_row(0, PLANE_COLS);
        while let Some(pkt) = sched.next_packet(&target, &shadow, tier, now) {
            assert_ne!(pkt.ctrl, CTRL_CLEAR_ROW);
            shadow.apply(&pkt, tier.payload_bytes());
        }
        assert_eq!(
            shadow.visual_row_text(0, PLANE_COLS),
            target.visual_row_cells(0)
        );
    }

    #[test]
    fn parses_oscquery_avatar_viewport_and_tier_matrix() {
        let tree_12r = serde_json::json!({
            "CONTENTS": {
                "change": {
                    "FULL_PATH": "/avatar/change",
                    "VALUE": ["avtr_demo_123"]
                },
                "parameters": {
                    "CONTENTS": {
                        "UniText_ProtoVer": {
                            "FULL_PATH": "/avatar/parameters/UniText_ProtoVer",
                            "VALUE": [1]
                        },
                        "UniText_D0": { "FULL_PATH": "/avatar/parameters/UniText_D0" },
                        "UniText_D15": { "FULL_PATH": "/avatar/parameters/UniText_D15" },
                        "UniText_Bank": { "FULL_PATH": "/avatar/parameters/UniText_Bank" },
                        "UniText_RowPage": { "FULL_PATH": "/avatar/parameters/UniText_RowPage" },
                        "UniText_Rows": {
                            "FULL_PATH": "/avatar/parameters/UniText_Rows",
                            "VALUE": [12]
                        },
                        "UniText_RowPx8": {
                            "FULL_PATH": "/avatar/parameters/UniText_RowPx8",
                            "VALUE": 32
                        },
                        "UniText_Width": {
                            "FULL_PATH": "/avatar/parameters/UniText_Width",
                            "VALUE": [0.25]
                        },
                        "UniText_Height": {
                            "FULL_PATH": "/avatar/parameters/UniText_Height",
                            "VALUE": [0.40]
                        },
                        "UniText_LocalClear": {
                            "FULL_PATH": "/avatar/parameters/UniText_LocalClear",
                            "VALUE": [true]
                        }
                    }
                }
            }
        });
        assert_eq!(
            parse_oscquery_avatar(&tree_12r),
            OscQueryAvatarInfo {
                tier: Some(UniTextTier::Turbo),
                proto_ver: Some(1),
                base_viewport: BaseViewport {
                    cols: 64,
                    base_rows: 12,
                    base_row_px: 256,
                    has_row_page: true,
                },
                width: 0.25,
                height: 0.40,
                viewport: ViewportConfig {
                    cols: 64,
                    rows: 12,
                    row_px: 288,
                    send_bank: true,
                    send_row_page: true,
                },
                has_unitext_params: true,
                has_resize_axes: true,
                avatar_id: Some("avtr_demo_123".into()),
                local_clear: true,
            }
        );

        // <=6 rows must keep send_row_page=false even if UniText_RowPage exists; narrow mode clamps row_px to 384.
        let tree_clamped = serde_json::json!({
            "CONTENTS": {
                "parameters": {
                    "CONTENTS": {
                        "UniText_D2": { "FULL_PATH": "/avatar/parameters/UniText_D2" },
                        "UniText_RowPage": { "FULL_PATH": "/avatar/parameters/UniText_RowPage" },
                        "UniText_Rows": {
                            "FULL_PATH": "/avatar/parameters/UniText_Rows",
                            "VALUE": [2.0]
                        },
                        "UniText_RowPx8": {
                            "FULL_PATH": "/avatar/parameters/UniText_RowPx8",
                            "VALUE": [99]
                        }
                    }
                }
            }
        });
        assert_eq!(
            parse_oscquery_avatar(&tree_clamped),
            OscQueryAvatarInfo {
                tier: Some(UniTextTier::Lite),
                proto_ver: None,
                base_viewport: BaseViewport {
                    cols: 32,
                    base_rows: 4,
                    base_row_px: 384,
                    has_row_page: true,
                },
                width: 0.0,
                height: 0.0,
                viewport: ViewportConfig {
                    cols: 32,
                    rows: 4,
                    row_px: 384,
                    send_bank: false,
                    send_row_page: false,
                },
                has_unitext_params: true,
                has_resize_axes: false,
                avatar_id: None,
                local_clear: false,
            }
        );

        // >6 rows without UniText_RowPage keeps send_row_page=false; rows clamp to 12; row_px8 clamps to 192.
        let tree_tall_no_rp = serde_json::json!({
            "CONTENTS": {
                "parameters": {
                    "CONTENTS": {
                        "UniText_D5": { "FULL_PATH": "/avatar/parameters/UniText_D5" },
                        "UniText_Rows": { "VALUE": 20 },
                        "UniText_RowPx8": { "VALUE": [10] }
                    }
                }
            }
        });
        assert_eq!(
            parse_oscquery_avatar(&tree_tall_no_rp),
            OscQueryAvatarInfo {
                tier: Some(UniTextTier::Balanced),
                proto_ver: None,
                base_viewport: BaseViewport {
                    cols: 32,
                    base_rows: 12,
                    base_row_px: 192,
                    has_row_page: false,
                },
                width: 0.0,
                height: 0.0,
                viewport: ViewportConfig {
                    cols: 32,
                    rows: 12,
                    row_px: 192,
                    send_bank: false,
                    send_row_page: false,
                },
                has_unitext_params: true,
                has_resize_axes: false,
                avatar_id: None,
                local_clear: false,
            }
        );

        // Wide avatar with base_row_px=512 scales up to 640 at w=1.0 and activates RowPage when h scales >6 rows.
        let tree_wide_640 = serde_json::json!({
            "CONTENTS": {
                "parameters": {
                    "CONTENTS": {
                        "UniText_D7": { "FULL_PATH": "/avatar/parameters/UniText_D7" },
                        "UniText_Bank": { "FULL_PATH": "/avatar/parameters/UniText_Bank" },
                        "UniText_RowPage": { "FULL_PATH": "/avatar/parameters/UniText_RowPage" },
                        "UniText_Rows": { "VALUE": [6] },
                        "UniText_RowPx8": { "VALUE": [64] },
                        "UniText_Width": { "VALUE": [1.0] },
                        "UniText_Height": { "VALUE": [0.5] }
                    }
                }
            }
        });
        assert_eq!(
            parse_oscquery_avatar(&tree_wide_640),
            OscQueryAvatarInfo {
                tier: Some(UniTextTier::Standard),
                proto_ver: None,
                base_viewport: BaseViewport {
                    cols: 64,
                    base_rows: 6,
                    base_row_px: 512,
                    has_row_page: true,
                },
                width: 1.0,
                height: 0.5,
                viewport: ViewportConfig {
                    cols: 64,
                    rows: 9,
                    row_px: 640,
                    send_bank: true,
                    send_row_page: true,
                },
                has_unitext_params: true,
                has_resize_axes: true,
                avatar_id: None,
                local_clear: false,
            }
        );

        assert_eq!(
            parse_oscquery_avatar(&serde_json::json!({ "CONTENTS": {} })),
            OscQueryAvatarInfo::default()
        );
    }

    #[test]
    fn resolves_live_viewport_piecewise_linear_formula() {
        assert_eq!(resolve_live_viewport(6, 384, -1.0, -1.0, true), (4, 192));
        assert_eq!(resolve_live_viewport(6, 384, -0.5, -0.5, true), (5, 288));
        assert_eq!(resolve_live_viewport(6, 384, 0.0, 0.0, true), (6, 384));
        assert_eq!(resolve_live_viewport(6, 384, 0.5, 0.5, true), (9, 480));
        assert_eq!(resolve_live_viewport(6, 384, 1.0, 1.0, true), (12, 576));

        // Base 8 rows, 512 px wide clamps row_px_max at 640.
        assert_eq!(resolve_live_viewport(8, 512, -1.0, -1.0, true), (4, 256));
        assert_eq!(resolve_live_viewport(8, 512, 0.0, -0.5, true), (6, 512));
        assert_eq!(resolve_live_viewport(8, 512, 0.5, 0.5, true), (10, 576));
        assert_eq!(resolve_live_viewport(8, 512, 1.0, 1.0, true), (12, 640));

        // Narrow 32-col mode clamps max width at 384 even for positive w.
        assert_eq!(resolve_live_viewport(6, 384, -1.0, 0.0, false), (6, 192));
        assert_eq!(resolve_live_viewport(6, 384, 0.0, 0.0, false), (6, 384));
        assert_eq!(resolve_live_viewport(6, 384, 1.0, 1.0, false), (12, 384));

        // round_to_8 rounding and NaN/out-of-range safety.
        assert_eq!(resolve_live_viewport(6, 392, -1.0, 0.0, true), (6, 200));
        assert_eq!(
            resolve_live_viewport(6, 384, f64::NAN, f64::INFINITY, true),
            (6, 384)
        );
        assert_eq!(resolve_live_viewport(6, 384, 5.0, -2.0, true), (4, 576));

        let base = BaseViewport {
            cols: WIDE_COLS,
            base_rows: 6,
            base_row_px: 384,
            has_row_page: true,
        };
        assert!(!base.resolve_live(0.0, 0.0).send_row_page);
        assert!(base.resolve_live(0.0, 0.5).send_row_page);
        let no_rp = BaseViewport {
            has_row_page: false,
            ..base
        };
        assert!(!no_rp.resolve_live(0.0, 0.5).send_row_page);
    }

    #[test]
    fn decodes_live_osc_resize_and_clear_messages_and_bundles() {
        let msg_w = encoder::encode(&OscPacket::Message(OscMessage {
            addr: OSC_PARAM_WIDTH.into(),
            args: vec![OscType::Float(0.75)],
        }))
        .unwrap();
        assert_eq!(
            decode_live_osc_events(&msg_w),
            vec![LiveOscParamEvent::Width(0.75)]
        );

        let msg_clamp = encoder::encode(&OscPacket::Message(OscMessage {
            addr: OSC_PARAM_HEIGHT.into(),
            args: vec![OscType::Double(-2.5)],
        }))
        .unwrap();
        assert_eq!(
            decode_live_osc_events(&msg_clamp),
            vec![LiveOscParamEvent::Height(-1.0)]
        );

        let msg_nan = encoder::encode(&OscPacket::Message(OscMessage {
            addr: OSC_PARAM_WIDTH.into(),
            args: vec![OscType::Float(f32::NAN)],
        }))
        .unwrap();
        assert!(decode_live_osc_events(&msg_nan).is_empty());

        let bundle = encoder::encode(&OscPacket::Bundle(OscBundle {
            timetag: OscTime {
                seconds: 0,
                fractional: 1,
            },
            content: vec![
                OscPacket::Message(OscMessage {
                    addr: OSC_PARAM_WIDTH.into(),
                    args: vec![OscType::Int(-1)],
                }),
                OscPacket::Message(OscMessage {
                    addr: OSC_PARAM_HEIGHT.into(),
                    args: vec![OscType::Float(0.5)],
                }),
                OscPacket::Message(OscMessage {
                    addr: OSC_PARAM_LOCAL_CLEAR.into(),
                    args: vec![OscType::Bool(true)],
                }),
                OscPacket::Message(OscMessage {
                    addr: "/avatar/parameters/Unrelated".into(),
                    args: vec![OscType::Int(1)],
                }),
            ],
        }))
        .unwrap();
        assert_eq!(
            decode_live_osc_events(&bundle),
            vec![
                LiveOscParamEvent::Width(-1.0),
                LiveOscParamEvent::Height(0.5),
                LiveOscParamEvent::LocalClear(true),
            ]
        );
    }

    #[test]
    fn osc_bundle_encodes_bank_row_page_and_ctrl_order() {
        let pkt = Packet {
            ctrl: 97,
            data: vec![72, 105, 0, 0, 0, 0],
            bank: 1,
            row_page: 1,
        };

        let bytes_plain = encode_osc_bundle(&pkt, 6, false, false).unwrap();
        let (_, decoded_plain) = rosc::decoder::decode_udp(&bytes_plain).unwrap();
        let OscPacket::Bundle(bundle_plain) = decoded_plain else {
            panic!("expected OSC bundle");
        };
        assert_eq!(bundle_plain.content.len(), 7);
        assert!(matches!(
            &bundle_plain.content[0],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_D0" && m.args == vec![OscType::Int(72)]
        ));
        assert!(matches!(
            &bundle_plain.content[6],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_Ctrl" && m.args == vec![OscType::Int(97)]
        ));

        let bytes_bank = encode_osc_bundle(&pkt, 6, true, false).unwrap();
        let (_, decoded_bank) = rosc::decoder::decode_udp(&bytes_bank).unwrap();
        let OscPacket::Bundle(bundle_bank) = decoded_bank else {
            panic!("expected OSC bundle");
        };
        assert_eq!(bundle_bank.content.len(), 8);
        assert!(matches!(
            &bundle_bank.content[6],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_Bank" && m.args == vec![OscType::Bool(true)]
        ));
        assert!(matches!(
            &bundle_bank.content[7],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_Ctrl" && m.args == vec![OscType::Int(97)]
        ));

        let bytes_both = encode_osc_bundle(&pkt, 6, true, true).unwrap();
        let (_, decoded_both) = rosc::decoder::decode_udp(&bytes_both).unwrap();
        let OscPacket::Bundle(bundle_both) = decoded_both else {
            panic!("expected OSC bundle");
        };
        assert_eq!(bundle_both.content.len(), 9);
        assert!(matches!(
            &bundle_both.content[6],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_Bank" && m.args == vec![OscType::Bool(true)]
        ));
        assert!(matches!(
            &bundle_both.content[7],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_RowPage" && m.args == vec![OscType::Bool(true)]
        ));
        assert!(matches!(
            &bundle_both.content[8],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_Ctrl" && m.args == vec![OscType::Int(97)]
        ));

        // Compact36 (n_bytes = 5): D0..D3 Int + UniText_D4b4..D4b7 Bool
        let pkt_c36 = Packet {
            ctrl: 145,
            data: vec![1, 2, 3, 4, 0xA0], // high nibble 0b1010 -> b4=false, b5=true, b6=false, b7=true
            bank: 1,
            row_page: 0,
        };
        let bytes_c36 = encode_osc_bundle(&pkt_c36, COMPACT36_BYTES, true, false).unwrap();
        let (_, decoded_c36) = rosc::decoder::decode_udp(&bytes_c36).unwrap();
        let OscPacket::Bundle(bundle_c36) = decoded_c36 else {
            panic!("expected OSC bundle");
        };
        assert_eq!(bundle_c36.content.len(), 10);
        assert!(matches!(
            &bundle_c36.content[3],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_D3" && m.args == vec![OscType::Int(4)]
        ));
        assert!(matches!(
            &bundle_c36.content[4],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_D4b4" && m.args == vec![OscType::Bool(false)]
        ));
        assert!(matches!(
            &bundle_c36.content[5],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_D4b5" && m.args == vec![OscType::Bool(true)]
        ));
        assert!(matches!(
            &bundle_c36.content[6],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_D4b6" && m.args == vec![OscType::Bool(false)]
        ));
        assert!(matches!(
            &bundle_c36.content[7],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_D4b7" && m.args == vec![OscType::Bool(true)]
        ));
        assert!(matches!(
            &bundle_c36.content[8],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_Bank" && m.args == vec![OscType::Bool(true)]
        ));
        assert!(matches!(
            &bundle_c36.content[9],
            OscPacket::Message(m) if m.addr == "/avatar/parameters/UniText_Ctrl" && m.args == vec![OscType::Int(145)]
        ));
    }

    #[test]
    fn diff_scheduler_refreshes_top_to_bottom_and_syncs_style_first() {
        let mut target = VirtualScreen::default();
        target.scroll_up(2);
        target.set_row_text(0, "Original line 1");
        target.set_row_style(0, 1, 1);
        target.set_row_text(1, "译文第一行");
        target.set_row_style(1, 2, 2);
        target.set_row_text(2, "Original line 2");
        target.set_row_style(2, 4, 0);

        let mut shadow = ShadowCrt::default();
        let mut sched = DiffScheduler::default();
        let now = Instant::now();
        let tier = UniTextTier::Balanced;
        let mut max_visual = 0usize;
        let mut written_visuals = Vec::new();

        for _ in 0..64 {
            let Some(pkt) = sched.next_packet(&target, &shadow, tier, now) else {
                break;
            };
            let prev_cells = shadow.cells;
            let is_text_write = matches!(pkt.ctrl, 1..=239 | 243..=249);
            shadow.apply(&pkt, tier.payload_bytes());
            if is_text_write {
                for v in 0..3 {
                    let phys = target.visual_to_physical(v);
                    let base_shadow = phys * WIDE_COLS;
                    if prev_cells[base_shadow..base_shadow + PLANE_COLS]
                        != shadow.cells[base_shadow..base_shadow + PLANE_COLS]
                    {
                        assert!(
                            v >= max_visual,
                            "expected top-to-bottom refresh, got visual row {v} after {max_visual}"
                        );
                        assert!(
                            shadow.known_styles[phys]
                                && shadow.row_styles[phys] == target.row_styles[phys],
                            "expected style synced before writing text on visual row {v}"
                        );
                        max_visual = v;
                        if written_visuals.last().copied() != Some(v) {
                            written_visuals.push(v);
                        }
                    }
                }
            }
        }

        assert_eq!(written_visuals, vec![0, 1, 2]);
        for r in 0..DEFAULT_ROWS {
            assert_eq!(
                shadow.visual_row_text(r, PLANE_COLS),
                target.visual_row_cells(r)
            );
            let phys = target.visual_to_physical(r);
            assert_eq!(shadow.row_styles[phys], target.row_styles[phys]);
        }
    }
}
