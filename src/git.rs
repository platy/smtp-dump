//! Helpers for git

use anyhow::{format_err, Context, Result};
use git2::{Commit, Oid, Repository, Signature, Tree, TreeBuilder};

pub struct CommitBuilder<'repo> {
    repo: &'repo Repository,
    tree_builder: TreeBuilder<'repo>,
    parent: Option<Commit<'repo>>,
}

impl<'repo> CommitBuilder<'repo> {
    /// Start building a commit on this repository
    pub fn new(repo: &'repo Repository, parent: Option<Commit<'repo>>) -> Result<Self, git2::Error> {
        let tree: Option<Tree<'_>> = parent.as_ref().map(Commit::tree).transpose()?;
        let tree_builder: TreeBuilder<'repo> = repo.treebuilder(tree.as_ref())?;
        Ok(CommitBuilder {
            repo,
            tree_builder,
            parent,
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
    pub fn commit(
        self,
        author: &Signature,
        committer: &Signature,
        message: &str,
    ) -> Result<Commit<'repo>, git2::Error> {
        let oid = self.tree_builder.write()?;
        let tree = self.repo.find_tree(oid)?;
        let oid = self.repo.commit(
            None,
            author,
            committer,
            message,
            &tree,
            self.parent.as_ref().map(|c| vec![c]).unwrap_or_default().as_slice(),
        )?;
        self.repo.find_commit(oid)
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
    let base = it.next().context("write_to_path_in_tree called with empty path")?;
    if let Some(rest) = it.next() {
        // make a tree node
        let child_tree = if let Some(child_entry) = tree_builder.get(base)? {
            let child_tree = child_entry.to_object(repo)?.into_tree();
            // handle the case where the tree that we want is a blob, we'll just add a symbol to the end of the name, we use "|" when a tree's name was blocked by a blob and "-" when a blobs name was blocked by a tree
            match child_tree {
                Ok(child_tree) => Some(child_tree),
                Err(_) => {
                    println!("Malformed a tree name to avoid collsion with a blob {}", path);
                    if let Some(malformed_entry) = tree_builder.get(format!("{}|", base))? {
                        Some(
                            malformed_entry
                                .to_object(repo)?
                                .into_tree()
                                .map_err(|_| format_err!("file blocking tree creation x 2 as {}", path))?,
                        )
                    } else {
                        None
                    }
                }
            }
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
