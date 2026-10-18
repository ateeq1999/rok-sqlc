pub mod writer;

pub use writer::{
    SCAFFOLD_HEADER, apply_files, compare_files, find_stale, is_generated_file, remove_stale,
    write_absent,
};
