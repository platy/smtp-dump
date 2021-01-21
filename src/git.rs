//! Helpers for git

use anyhow::{format_err, Context, Result};
use git2::{Commit, Oid, Reference, Repository, Signature, Tree, TreeBuilder};

pub struct CommitBuilder<'repo> {
    repo: &'repo Repository,
    tree_builder: TreeBuilder<'repo>,
    r#ref: String,
}

impl<'repo> CommitBuilder<'repo> {
    /// Start building a commit on this repository with the specified ref, if the ref doesn't currently exist it will be created
    pub fn new(repo: &'repo Repository, r#ref: &str) -> Result<Self> {
        let tree: Option<Tree<'_>> = find_optional_reference(repo, r#ref)?
            .as_ref()
            .map(Reference::peel_to_tree)
            .transpose()?;
        let tree_builder: TreeBuilder<'repo> = repo.treebuilder(tree.as_ref())?;
        Ok(CommitBuilder {
            repo,
            tree_builder,
            r#ref: r#ref.to_owned(),
        })
    }

    pub fn add_to_tree(&mut self, path: &str, oid: Oid, file_mode: i32) -> Result<()> {
        write_to_path_in_tree(
            self.repo,
            &mut self.tree_builder,
            path.strip_prefix('/').context("relative path provided")?,
            oid,
            file_mode,
        )
    }

    /// Writes the built tree, a comit for it and updates the ref
    pub fn commit(self, author: &Signature, committer: &Signature, message: &str) -> Result<()> {
        let oid = self.tree_builder.write()?;
        let tree = self.repo.find_tree(oid)?;
        let parents: Option<Commit> = find_optional_reference(self.repo, &self.r#ref)?
            .as_ref()
            .map(Reference::peel_to_commit)
            .transpose()?;
        self.repo.commit(
            Some(self.r#ref.as_str()),
            &author,
            &committer,
            message,
            &tree,
            parents.as_ref().map(|c| vec![c]).unwrap_or_default().as_slice(),
        )?;
        Ok(())
    }
}

/// recursively build tree nodes and add the blob
/// Path should be relative
/// The key filemodes are 0o100644 for a file, 0o100755 for an executable, 0o040000 for a tree and 0o120000 or 0o160000?
fn write_to_path_in_tree(
    repo: &Repository,
    tree_builder: &mut TreeBuilder,
    path: &str,
    oid: Oid,
    filemode: i32,
) -> Result<()> {
    let mut it = path.splitn(2, '/');
    let base = it.next().unwrap();
    if let Some(rest) = it.next() {
        // make a tree node
        let child_tree = if let Some(child_entry) = tree_builder.get(base)? {
            let child_tree = child_entry
                .to_object(repo)?
                .into_tree()
                .map_err(|_| format_err!("file blocking tree creation at {}", path))?;
            Some(child_tree)
        } else {
            None
        };
        let mut child_tree_builder = repo.treebuilder(child_tree.as_ref())?;
        write_to_path_in_tree(repo, &mut child_tree_builder, rest, oid, filemode)?;
        let oid = child_tree_builder.write()?;
        tree_builder.insert(base, oid, 0o040000)?;
    } else {
        tree_builder.insert(base, oid, filemode)?;
    }
    Ok(())
}

fn find_optional_reference<'r>(repo: &'r Repository, name: &str) -> Result<Option<Reference<'r>>, git2::Error> {
    match repo.find_reference(name).map(Some) {
        Err(e) if e.code() == git2::ErrorCode::NotFound => Ok(None),
        r => r,
    }
}
