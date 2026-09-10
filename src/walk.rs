// Copyright (C) 2025 Ethan Uppal.
//
// This file is part of spadefmt.
//
// spadefmt is free software: you can redistribute it and/or modify it under the
// terms of the GNU General Public License as published by the Free Software
// Foundation, version 3 of the License only. spadefmt is distributed in the
// hope that it will be useful, but WITHOUT ANY WARRANTY; without even the
// implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
// the GNU General Public License for more details. You should have received a
// copy of the GNU General Public License along with spadefmt. If not, see
// <https://www.gnu.org/licenses/>.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// Every `.spade` file under `directory`, recursively, sorted by path.
/// Symlinked directories are not followed (a cycle would never end);
/// symlinked files are included.
pub fn spade_files(directory: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = vec![];
    collect(directory, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect(directory: &Path, into: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect(&path, into)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "spade")
            && (!file_type.is_symlink() || path.is_file())
        {
            into.push(path);
        }
    }
    Ok(())
}
