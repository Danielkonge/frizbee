use crate::r#const::*;
use smith_waterman_macro::generate_smith_waterman;
use std::ops::{BitAnd, BitOr, Not};
use std::simd::cmp::*;
use std::simd::{Mask, Simd};

generate_smith_waterman!(4);
generate_smith_waterman!(8);
generate_smith_waterman!(12);
generate_smith_waterman!(16);
generate_smith_waterman!(24);
generate_smith_waterman!(32);
generate_smith_waterman!(48);
generate_smith_waterman!(64);
generate_smith_waterman!(96);
generate_smith_waterman!(128);
generate_smith_waterman!(160);
generate_smith_waterman!(192);
generate_smith_waterman!(224);
generate_smith_waterman!(256);
generate_smith_waterman!(384);
generate_smith_waterman!(512);

pub fn interleave_strings(strings: &[&str]) -> [[u8; SIMD_WIDTH]; 8] {
    let mut cased_result = [[0; SIMD_WIDTH]; 8];

    for (char_idx, cased_slice) in cased_result.iter_mut().enumerate() {
        for str_idx in 0..SIMD_WIDTH {
            if let Some(char) = strings[str_idx].as_bytes().get(char_idx) {
                cased_slice[str_idx] = *char;
            }
        }
    }

    cased_result
}

type SimdVec = Simd<u8, SIMD_WIDTH>;

pub fn smith_waterman_inter_simd(needle: &str, haystacks: &[&str]) -> [u16; SIMD_WIDTH] {
    let needle_str = needle;
    let needle = needle.as_bytes();
    let needle_len = needle.len();
    let haystack_len = haystacks.iter().map(|x| x.len()).max().unwrap();

    let haystack = interleave_strings(haystacks);

    // State
    let mut prev_col_score_simds: [SimdVec; 9] = [Simd::splat(0); 9];
    let mut left_gap_penalty_masks = [Mask::splat(true); 8];
    let mut all_time_max_score = Simd::splat(0);

    // Delimiters
    let space_delimiter = " ".bytes().next().unwrap() as u8;
    let slash_delimiter = "/".bytes().next().unwrap() as u8;
    let dot_delimiter = ".".bytes().next().unwrap() as u8;
    let comma_delimiter = ",".bytes().next().unwrap() as u8;
    let underscore_delimiter = "_".bytes().next().unwrap() as u8;
    let dash_delimiter = "-".bytes().next().unwrap() as u8;
    let mut delimiters_arr = [dash_delimiter; SIMD_WIDTH];
    delimiters_arr[0] = space_delimiter;
    delimiters_arr[1] = slash_delimiter;
    delimiters_arr[2] = dot_delimiter;
    delimiters_arr[3] = comma_delimiter;
    delimiters_arr[4] = underscore_delimiter;
    delimiters_arr[5] = dash_delimiter; // repeat for the rest of the array
    let delimiters = Simd::from_array(delimiters_arr);
    let delimiter_bonus = Simd::splat(DELIMITER_BONUS);

    // Capitalization
    let capital_start = Simd::splat("A".bytes().next().unwrap() as u8);
    let capital_end = Simd::splat("Z".bytes().next().unwrap() as u8);
    let capitalization_bonus = Simd::splat(CAPITALIZATION_BONUS);
    let to_lowercase_mask = Simd::splat(0x20);

    // Scoring params
    let gap_open_penalty = Simd::splat(GAP_OPEN_PENALTY);
    let gap_extend_penalty = Simd::splat(GAP_EXTEND_PENALTY);

    let match_score = Simd::splat(MATCH_SCORE);
    let mismatch_score = Simd::splat(MISMATCH_PENALTY);
    let prefix_match_score = Simd::splat(MATCH_SCORE + PREFIX_BONUS);
    let first_char_match_score = Simd::splat(MATCH_SCORE * FIRST_CHAR_MULTIPLIER);
    let first_char_prefix_match_score =
        Simd::splat((MATCH_SCORE + PREFIX_BONUS) * FIRST_CHAR_MULTIPLIER);

    let zero: SimdVec = Simd::splat(0);

    for i in 1..=needle_len {
        let match_score = if i == 1 {
            first_char_match_score
        } else {
            match_score
        };
        let prefix_match_score = if i == 1 {
            first_char_prefix_match_score
        } else {
            prefix_match_score
        };

        let needle_char = Simd::splat(needle[i - 1]);
        let mut up_score_simd = Simd::splat(0);
        let mut up_gap_penalty_mask = Mask::splat(true);
        let mut curr_col_score_simds: [SimdVec; 9] = [Simd::splat(0); 9];

        let needle_char_is_delimiter = delimiters.simd_eq(needle_char).any();
        let delimiter_bonus = if needle_char_is_delimiter {
            delimiter_bonus
        } else {
            zero
        };

        for j in 1..=haystack_len {
            let prefix_mask = Mask::splat(j == 1);
            // Load chunk and remove casing
            let cased_haystack_simd = SimdVec::from_array(haystack[j - 1]);
            let capital_mask = cased_haystack_simd
                .simd_ge(capital_start)
                .bitand(cased_haystack_simd.simd_le(capital_end));
            let haystack_simd = cased_haystack_simd | capital_mask.select(to_lowercase_mask, zero);

            // Give a bonus for prefix matches
            let match_score = prefix_mask.select(prefix_match_score, match_score);

            // Calculate diagonal (match/mismatch) scores
            let diag = prev_col_score_simds[j - 1];
            let match_mask = needle_char.simd_eq(haystack_simd);
            let diag_score = match_mask.select(
                diag + match_score
                    + delimiter_bonus
                    // XOR with prefix mask to ignore capitalization on the prefix
                    + capital_mask.bitand(prefix_mask.not()).select(capitalization_bonus, zero),
                zero.simd_max(diag - mismatch_score),
            );

            // Load and calculate up scores
            let up_gap_penalty = up_gap_penalty_mask.select(gap_open_penalty, gap_extend_penalty);
            let up_score = zero.simd_max(up_score_simd - up_gap_penalty);

            // Load and calculate left scores
            let left = prev_col_score_simds[j];
            let left_gap_penalty_mask = left_gap_penalty_masks[j - 1];
            let left_gap_penalty =
                left_gap_penalty_mask.select(gap_open_penalty, gap_extend_penalty);
            let left_score = zero.simd_max(left - left_gap_penalty);

            // Calculate maximum scores
            // Note up_score and left_score are >= 0, so max_score >= 0
            let max_score: SimdVec = diag_score.simd_max(up_score).simd_max(left_score);

            // Update gap penalty mask
            let diag_mask = max_score.simd_eq(diag_score);
            up_gap_penalty_mask = max_score.simd_ne(up_score).bitor(diag_mask);
            left_gap_penalty_masks[j - 1] = max_score.simd_ne(left_score).bitor(diag_mask);

            // Store the scores for the next iterations
            up_score_simd = max_score;
            curr_col_score_simds[j] = max_score;

            // Store the maximum score across all runs
            all_time_max_score = all_time_max_score.simd_max(max_score);
        }

        prev_col_score_simds = curr_col_score_simds;
    }

    let mut max_scores_vec = [0; SIMD_WIDTH];
    for i in 0..SIMD_WIDTH {
        max_scores_vec[i] = all_time_max_score[i] as u16;
        if haystacks[i] == needle_str {
            max_scores_vec[i] += EXACT_MATCH_BONUS as u16;
        }
    }
    max_scores_vec
}

//pub fn char_indices_from_scores(
//    score_matrices: &[SimdScoreVec],
//    max_scores: &[u8; SIMD_WIDTH],
//    haystack_len: usize,
//) -> Vec<Vec<usize>> {
//    // Get the row and column indices of the maximum score
//    let max_scores = Simd::from_slice(max_scores);
//    let mut max_row = Simd::splat(0);
//    let mut max_col = Simd::splat(0);
//
//    for (col_idx, column) in score_matrices.chunks_exact(haystack_len).enumerate() {
//        let col_idx_simd = Simd::splat(col_idx as u8);
//        for (row_idx, score) in column.iter().enumerate() {
//            let row_idx_simd = Simd::splat(row_idx as u8);
//
//            let eq = score.simd_eq(max_scores);
//            max_row = eq.select(row_idx_simd, max_row);
//            max_col = eq.select(col_idx_simd, max_col);
//        }
//    }
//
//    let max_row_arr = max_row.to_array();
//    let max_col_arr = max_col.to_array();
//    let max_score_positions = max_row_arr
//        .iter()
//        .zip(max_col_arr.iter())
//        .map(|(row, col)| (*row as usize, *col as usize));
//
//    // Traceback and store the indices
//    let mut indices = vec![HashSet::new(); SIMD_WIDTH];
//    let row_stride = haystack_len + 1;
//    for (idx, (row_idx, col_idx)) in max_score_positions.enumerate() {
//        let indices = &mut indices[idx];
//        indices.insert(col_idx);
//
//        let mut last_idx = (row_idx, col_idx);
//        let mut score = score_matrices[row_idx * row_stride + col_idx][idx];
//        while score > 0 {
//            let (row_idx, col_idx) = last_idx;
//
//            // Gather up the scores for all possible paths
//            let diag = score_matrices[(row_idx - 1) * row_stride + col_idx - 1][idx];
//            let up = score_matrices[(row_idx - 1) * row_stride + col_idx][idx];
//            let left = score_matrices[row_idx * row_stride + col_idx - 1][idx];
//
//            // Choose the best path and store the index on the haystack if applicable
//            // TODO: is this logic correct? which route should we prefer?
//            score = diag.max(up).max(left);
//            if score == diag {
//                indices.insert(col_idx - 1);
//                last_idx = (row_idx - 1, col_idx - 1);
//            } else if score == up {
//                indices.insert(col_idx - 1);
//                last_idx = (row_idx, col_idx - 1);
//            } else {
//                last_idx = (row_idx - 1, col_idx);
//            }
//        }
//    }
//
//    indices
//        .iter()
//        .map(|indices| indices.iter().copied().collect())
//        .collect()
//}
