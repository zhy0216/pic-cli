use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    ErrorCode, PicError, Result,
    document::{DOCUMENT_SCHEMA_VERSION, PIXEL_SEMANTICS, RevisionId},
    limits::ResourceLimits,
    operation::OperationSpec,
};

use super::storage::{self, Storage};

pub const INITIAL_REVISION: &str = "r0";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetRef {
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitRef {
    pub commit_id: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    pub pixel_semantics: String,
    pub engine_version: String,
    pub source: AssetRef,
    /// Provenance only, never a file dependency or a path to resolve.
    pub imported_from: String,
    pub initial_revision: RevisionId,
    pub current_revision: RevisionId,
    /// Redo follows only this tip's ancestry. All commits remain readable.
    pub tip_revision: RevisionId,
    pub commits: Vec<CommitRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommittedOp {
    pub op_id: String,
    pub base_revision: RevisionId,
    pub revision: RevisionId,
    pub operation: OperationSpec,
    pub input_assets: Vec<AssetRef>,
    pub result_assets: Vec<AssetRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Commit {
    pub schema_version: u32,
    pub commit_id: String,
    pub base_revision: RevisionId,
    pub revision: RevisionId,
    pub steps: Vec<CommittedOp>,
}

pub(super) fn invalid(message: impl Into<String>) -> PicError {
    PicError::new(ErrorCode::InvalidProject, message)
}

pub(super) fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    // Check the structural version before interpreting the rest of a future format.
    #[derive(Deserialize)]
    struct Header {
        schema_version: u32,
    }
    let header: Header =
        serde_json::from_slice(bytes).map_err(|e| invalid(format!("project JSON: {e}")))?;
    if header.schema_version != DOCUMENT_SCHEMA_VERSION {
        return Err(PicError::new(
            ErrorCode::UnsupportedVersion,
            format!(
                "unsupported project schema_version {}; expected {DOCUMENT_SCHEMA_VERSION}",
                header.schema_version
            ),
        ));
    }
    serde_json::from_slice(bytes).map_err(|e| invalid(format!("project JSON: {e}")))
}

pub(super) struct History {
    pub commits: Vec<Commit>,
    revisions: BTreeMap<String, (usize, usize)>,
    pub metadata_bytes: u64,
}

impl History {
    pub fn load(
        storage: &Storage,
        manifest: &Manifest,
        manifest_bytes: u64,
        limits: &ResourceLimits,
    ) -> Result<Self> {
        if manifest.pixel_semantics != PIXEL_SEMANTICS {
            return Err(PicError::new(
                ErrorCode::UnsupportedVersion,
                format!("unsupported pixel_semantics '{}'", manifest.pixel_semantics),
            ));
        }
        storage::check_hash(&manifest.source.sha256)?;
        if manifest.source.bytes > limits.max_input_bytes {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "source asset exceeds byte limit",
            ));
        }
        if manifest.initial_revision.0 != INITIAL_REVISION {
            return Err(invalid("invalid initial revision"));
        }
        if manifest.commits.len() > limits.max_history_operations {
            return Err(PicError::new(
                ErrorCode::ResourceLimit,
                "project exceeds history limit",
            ));
        }
        let mut history = Self {
            commits: Vec::new(),
            revisions: BTreeMap::new(),
            metadata_bytes: manifest_bytes,
        };
        for (index, reference) in manifest.commits.iter().enumerate() {
            let bytes = storage.commit(
                &reference.sha256,
                limits
                    .max_project_bytes
                    .saturating_sub(history.metadata_bytes),
            )?;
            history.metadata_bytes += bytes.len() as u64;
            let commit: Commit = parse(&bytes)?;
            if commit.commit_id != format!("c{}", index + 1)
                || reference.commit_id != commit.commit_id
            {
                return Err(invalid(
                    "commit IDs must be unique and match the manifest order",
                ));
            }
            history
                .require_revision(&commit.base_revision.0)
                .map_err(|_| invalid("commit base is not an earlier committed revision"))?;
            if commit.steps.is_empty() || commit.steps.len() > limits.max_operations {
                return Err(invalid("commit must contain 1..max_operations steps"));
            }
            if commit.steps.len()
                > limits
                    .max_history_operations
                    .saturating_sub(history.revisions.len())
            {
                return Err(PicError::new(
                    ErrorCode::ResourceLimit,
                    "project exceeds history operation limit",
                ));
            }
            let mut base = &commit.base_revision;
            for (step_index, step) in commit.steps.iter().enumerate() {
                let number = history.revisions.len() + 1;
                if step.op_id != format!("op{number}")
                    || step.revision.0 != format!("r{number}")
                    || &step.base_revision != base
                {
                    return Err(invalid("invalid operation identity or revision chain"));
                }
                if step.input_assets != [manifest.source.clone()] || !step.result_assets.is_empty()
                {
                    return Err(invalid(
                        "single-image operations must depend on the source asset and have no result assets",
                    ));
                }
                let normalized = step.operation.validate()?.to_spec();
                if serde_json::to_value(&normalized).map_err(|e| invalid(e.to_string()))?
                    != serde_json::to_value(&step.operation).map_err(|e| invalid(e.to_string()))?
                {
                    return Err(invalid("committed operation parameters are not normalized"));
                }
                history
                    .revisions
                    .insert(step.revision.0.clone(), (index, step_index));
                base = &step.revision;
            }
            if base != &commit.revision {
                return Err(invalid("commit output must be its last step revision"));
            }
            history.commits.push(commit);
        }
        history
            .require_revision(&manifest.tip_revision.0)
            .map_err(|_| invalid("tip revision is not committed"))?;
        if !history
            .boundaries(&manifest.tip_revision.0)?
            .contains(&manifest.current_revision.0)
        {
            return Err(invalid(
                "current revision is not a group boundary on the active history",
            ));
        }
        Ok(history)
    }

    pub fn len(&self) -> usize {
        self.revisions.len()
    }

    pub fn require_revision(&self, revision: &str) -> Result<()> {
        if revision == INITIAL_REVISION || self.revisions.contains_key(revision) {
            Ok(())
        } else {
            Err(PicError::new(
                ErrorCode::RevisionNotFound,
                format!("revision '{revision}' is not committed"),
            ))
        }
    }

    pub fn step(&self, revision: &str) -> Option<(&Commit, &CommittedOp)> {
        self.revisions
            .get(revision)
            .map(|&(commit, step)| (&self.commits[commit], &self.commits[commit].steps[step]))
    }

    pub fn path(&self, revision: &str) -> Result<Vec<&CommittedOp>> {
        self.require_revision(revision)?;
        let mut steps = Vec::new();
        let mut cursor = revision;
        while let Some((_, step)) = self.step(cursor) {
            steps.push(step);
            cursor = &step.base_revision.0;
        }
        steps.reverse();
        Ok(steps)
    }

    /// A fork from the middle of a group truncates that group on the active path only.
    pub fn boundaries(&self, revision: &str) -> Result<Vec<String>> {
        let steps = self.path(revision)?;
        let mut boundaries = vec![INITIAL_REVISION.to_owned()];
        for (index, step) in steps.iter().enumerate() {
            let group = self.revisions[&step.revision.0].0;
            if steps
                .get(index + 1)
                .is_none_or(|next| self.revisions[&next.revision.0].0 != group)
            {
                boundaries.push(step.revision.0.clone());
            }
        }
        Ok(boundaries)
    }

    pub fn active_revisions(&self, tip: &str) -> Result<BTreeSet<String>> {
        let mut active = BTreeSet::from([INITIAL_REVISION.to_owned()]);
        active.extend(self.path(tip)?.iter().map(|step| step.revision.0.clone()));
        Ok(active)
    }
}
