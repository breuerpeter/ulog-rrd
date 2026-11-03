//! Rows into chunks: static rows one chunk each, time series as columns, a chunk per few thousand rows.

use re_chunk::{Chunk, ChunkId, RowId, TimeColumn, TimePoint};
use re_log_types::{EntityPath, TimelineName};
use re_sdk_types::{AsComponents, SerializedComponentColumn};

use crate::{err, Error, Options};

/// Rows per chunk: big enough to amortize the chunk header, small enough for the viewer's index.
const ROWS_PER_CHUNK: usize = 4096;

pub struct Writer<'a> {
    opts: &'a Options,
    timeline: TimelineName,
    chunks: Vec<Chunk>,
}

impl<'a> Writer<'a> {
    pub fn new(opts: &'a Options) -> Result<Self, Error> {
        let timeline = TimelineName::try_new(&opts.timeline).map_err(err)?;
        Ok(Self { opts, timeline, chunks: Vec::new() })
    }

    /// The pose of the prefix root in the caller's world, translation and quaternion (x, y, z, w),
    /// identity where the options leave it; `None` without a prefix.
    pub fn frame(&self) -> Option<([f32; 3], [f32; 4])> {
        self.opts
            .entity_path_prefix
            .as_ref()
            .map(|_| (self.opts.translation.unwrap_or([0.0; 3]), self.opts.rotation.unwrap_or([0.0, 0.0, 0.0, 1.0])))
    }

    pub fn scalars(&self) -> bool {
        self.opts.scalars
    }

    fn path(&self, path: &str) -> EntityPath {
        match &self.opts.entity_path_prefix {
            Some(prefix) => EntityPath::parse_forgiving(&format!("{}/{path}", prefix.trim_end_matches('/'))),
            None => EntityPath::parse_forgiving(path),
        }
    }

    /// A ULog timestamp on the recording's timeline, in nanoseconds.
    fn nanos(&self, timestamp_us: u64) -> i64 {
        (timestamp_us as i64 - self.opts.time_offset_us) * 1000
    }

    /// One row at no time: the value holds for the whole recording.
    pub fn static_row(&mut self, path: &str, archetype: &dyn AsComponents) -> Result<(), Error> {
        let chunk = Chunk::builder(self.path(path))
            .with_archetype(RowId::new(), TimePoint::default(), archetype)
            .build()
            .map_err(err)?;
        self.chunks.push(chunk);
        Ok(())
    }

    /// One row per timestamp, from columns that hold every row's value: the output of an archetype's
    /// `columns`, each partitioned into one element per row. Long series split into several chunks.
    pub fn columns(
        &mut self,
        path: &str,
        timestamps: &[u64],
        columns: impl Iterator<Item = SerializedComponentColumn>,
    ) -> Result<(), Error> {
        let columns: Vec<SerializedComponentColumn> = columns.collect();
        let entity = self.path(path);
        for start in (0..timestamps.len()).step_by(ROWS_PER_CHUNK) {
            let len = ROWS_PER_CHUNK.min(timestamps.len() - start);
            let time = TimeColumn::new_duration_nanos(
                self.timeline,
                timestamps[start..start + len].iter().map(|&t| self.nanos(t)),
            );
            let chunk = Chunk::from_auto_row_ids(
                ChunkId::new(),
                entity.clone(),
                std::iter::once((self.timeline, time)).collect(),
                columns.iter().map(|c| (c.descriptor.clone(), c.list_array.slice(start, len))).collect(),
            )
            .map_err(err)?;
            self.chunks.push(chunk);
        }
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<Chunk>, Error> {
        Ok(self.chunks)
    }
}
