//! Typed repositories. Only this module issues SQL against the pool.

use crate::Database;
use ariaflow_core::error::{AriaFlowError, Result};
use ariaflow_core::service::DownloadCounts;
use ariaflow_core::status::DownloadStatus;
use ariaflow_core::{
    AuthConfig, Checksum, Download, DownloadId, ProxyConfig, SegmentInfo, SegmentStatus,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::Row;

fn map_err(e: sqlx::Error) -> AriaFlowError {
    AriaFlowError::Storage(e.to_string())
}

fn dt_to_str(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339()
}

fn dt_from_str(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

fn opt_dt_to_str(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.as_ref().map(dt_to_str)
}

fn i64_to_u64(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

fn u64_to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

pub struct DownloadRepository {
    db: Database,
}

impl DownloadRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn insert(&self, d: &Download) -> Result<()> {
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;
        sqlx::query(
            "INSERT INTO downloads (
                id, url, final_url, filename, save_path, total_size, downloaded_size,
                status, progress, speed, average_speed, priority, retry_count,
                max_connections, current_connections, category, etag, last_modified,
                checksum_json, proxy_json, auth_json, error_message, create_subdirs,
                created_at, started_at, completed_at,
                limit_rate_bytes_per_sec, max_retries, scheduled_start_at,
                user_agent, ssh_certificate_id
            ) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(d.id.to_string())
        .bind(&d.url)
        .bind(&d.final_url)
        .bind(&d.filename)
        .bind(d.save_path.to_string_lossy().as_ref())
        .bind(d.total_size.map(u64_to_i64))
        .bind(u64_to_i64(d.downloaded_size))
        .bind(d.status.as_str())
        .bind(d.progress)
        .bind(u64_to_i64(d.speed_bytes_per_sec))
        .bind(u64_to_i64(d.average_speed_bytes_per_sec))
        .bind(d.priority as i64)
        .bind(d.retry_count as i64)
        .bind(d.max_connections as i64)
        .bind(d.current_connections as i64)
        .bind(&d.category)
        .bind(&d.etag)
        .bind(&d.last_modified)
        .bind(
            d.checksum
                .as_ref()
                .map(|c| serde_json::to_string(c).unwrap_or_default()),
        )
        .bind(
            d.proxy
                .as_ref()
                .map(|p| serde_json::to_string(p).unwrap_or_default()),
        )
        .bind(
            d.auth
                .as_ref()
                .map(|a| serde_json::to_string(a).unwrap_or_default()),
        )
        .bind(&d.error_message)
        .bind(d.create_subdirs as i64)
        .bind(dt_to_str(&d.created_at))
        .bind(opt_dt_to_str(&d.started_at))
        .bind(opt_dt_to_str(&d.completed_at))
        .bind(d.limit_rate_bytes_per_sec.map(u64_to_i64))
        .bind(d.max_retries.map(i64::from))
        .bind(opt_dt_to_str(&d.scheduled_start_at))
        .bind(&d.user_agent)
        .bind(&d.ssh_certificate_id)
        .execute(&mut *tx)
        .await
        .map_err(map_err)?;

        // Headers are part of the task record; replace existing set.
        sqlx::query("DELETE FROM download_headers WHERE download_id = ?")
            .bind(d.id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;
        for (name, value) in &d.headers {
            sqlx::query("INSERT INTO download_headers(download_id, name, value) VALUES (?,?,?)")
                .bind(d.id.to_string())
                .bind(name)
                .bind(value)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?;
        }
        tx.commit().await.map_err(map_err)?;
        Ok(())
    }

    fn row_to_download(
        &self,
        row: &sqlx::sqlite::SqliteRow,
        headers: Vec<(String, String)>,
    ) -> Result<Download> {
        let status_str: String = row.try_get("status").map_err(map_err)?;
        let checksum_json: Option<String> = row.try_get("checksum_json").map_err(map_err)?;
        let proxy_json: Option<String> = row.try_get("proxy_json").map_err(map_err)?;
        let auth_json: Option<String> = row.try_get("auth_json").map_err(map_err)?;
        let id_str: String = row.try_get("id").map_err(map_err)?;
        Ok(Download {
            id: id_str.parse()?,
            url: row.try_get("url").map_err(map_err)?,
            final_url: row.try_get("final_url").map_err(map_err)?,
            filename: row.try_get("filename").map_err(map_err)?,
            save_path: std::path::PathBuf::from(
                row.try_get::<String, _>("save_path").map_err(map_err)?,
            ),
            total_size: row
                .try_get::<Option<i64>, _>("total_size")
                .map_err(map_err)?
                .map(i64_to_u64),
            downloaded_size: i64_to_u64(row.try_get("downloaded_size").map_err(map_err)?),
            status: DownloadStatus::parse(&status_str).ok_or_else(|| {
                AriaFlowError::Storage(format!("unknown status in db: {status_str}"))
            })?,
            speed_bytes_per_sec: i64_to_u64(row.try_get("speed").map_err(map_err)?),
            average_speed_bytes_per_sec: i64_to_u64(row.try_get("average_speed").map_err(map_err)?),
            progress: row.try_get::<f64, _>("progress").map_err(map_err)?,
            priority: row.try_get::<i64, _>("priority").map_err(map_err)? as i32,
            retry_count: row.try_get::<i64, _>("retry_count").map_err(map_err)? as u32,
            max_connections: row.try_get::<i64, _>("max_connections").map_err(map_err)? as u16,
            current_connections: row
                .try_get::<i64, _>("current_connections")
                .map_err(map_err)? as u16,
            category: row.try_get("category").map_err(map_err)?,
            etag: row.try_get("etag").map_err(map_err)?,
            last_modified: row.try_get("last_modified").map_err(map_err)?,
            checksum: checksum_json.and_then(|s| serde_json::from_str::<Checksum>(&s).ok()),
            proxy: proxy_json.and_then(|s| serde_json::from_str::<ProxyConfig>(&s).ok()),
            auth: auth_json.and_then(|s| serde_json::from_str::<AuthConfig>(&s).ok()),
            headers,
            create_subdirs: row.try_get::<i64, _>("create_subdirs").map_err(map_err)? != 0,
            created_at: dt_from_str(&row.try_get::<String, _>("created_at").map_err(map_err)?),
            started_at: row
                .try_get::<Option<String>, _>("started_at")
                .map_err(map_err)?
                .map(|s| dt_from_str(&s)),
            completed_at: row
                .try_get::<Option<String>, _>("completed_at")
                .map_err(map_err)?
                .map(|s| dt_from_str(&s)),
            error_message: row.try_get("error_message").map_err(map_err)?,
            segments: Vec::new(),
            limit_rate_bytes_per_sec: row
                .try_get::<Option<i64>, _>("limit_rate_bytes_per_sec")
                .map_err(map_err)?
                .map(i64_to_u64),
            max_retries: row
                .try_get::<Option<i64>, _>("max_retries")
                .map_err(map_err)?
                .map(|v| v.clamp(0, u32::MAX as i64) as u32),
            scheduled_start_at: row
                .try_get::<Option<String>, _>("scheduled_start_at")
                .map_err(map_err)?
                .map(|s| dt_from_str(&s)),
            user_agent: row.try_get("user_agent").map_err(map_err)?,
            ssh_certificate_id: row.try_get("ssh_certificate_id").map_err(map_err)?,
        })
    }

    async fn headers_for(
        &self,
        ids: &[String],
    ) -> Result<std::collections::HashMap<String, Vec<(String, String)>>> {
        let mut map: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();
        if ids.is_empty() {
            return Ok(map);
        }
        // IN-list is safe here: ids are UUIDs, but bind them as parameters anyway.
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!("SELECT download_id, name, value FROM download_headers WHERE download_id IN ({placeholders})");
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        let rows = q.fetch_all(self.db.pool()).await.map_err(map_err)?;
        for row in rows {
            let download_id: String = row.try_get("download_id").map_err(map_err)?;
            let name: String = row.try_get("name").map_err(map_err)?;
            let value: String = row.try_get("value").map_err(map_err)?;
            map.entry(download_id).or_default().push((name, value));
        }
        Ok(map)
    }

    async fn get_row(&self, id: &DownloadId) -> Result<Option<sqlx::sqlite::SqliteRow>> {
        sqlx::query("SELECT * FROM downloads WHERE id = ?")
            .bind(id.to_string())
            .fetch_optional(self.db.pool())
            .await
            .map_err(map_err)
    }

    pub async fn get(&self, id: &DownloadId) -> Result<Option<Download>> {
        let row = match self.get_row(id).await? {
            Some(r) => r,
            None => return Ok(None),
        };
        let headers = self
            .headers_for(&[id.to_string()])
            .await?
            .remove(&id.to_string())
            .unwrap_or_default();
        Ok(Some(self.row_to_download(&row, headers)?))
    }

    pub async fn list(&self) -> Result<Vec<Download>> {
        let rows = sqlx::query("SELECT * FROM downloads ORDER BY created_at DESC")
            .fetch_all(self.db.pool())
            .await
            .map_err(map_err)?;
        let ids: Vec<String> = rows.iter().map(|r| r.get::<String, _>("id")).collect();
        let mut header_map = self.headers_for(&ids).await?;
        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let id: String = row.try_get("id").map_err(map_err)?;
            let headers = header_map.remove(&id).unwrap_or_default();
            out.push(self.row_to_download(row, headers)?);
        }
        Ok(out)
    }

    pub async fn update_status(
        &self,
        id: &DownloadId,
        status: DownloadStatus,
        error_message: Option<&str>,
    ) -> Result<bool> {
        let now = dt_to_str(&Utc::now());
        let result = sqlx::query(
            "UPDATE downloads SET
                status = ?,
                error_message = COALESCE(?, error_message),
                started_at = CASE WHEN ? = 'downloading' AND started_at IS NULL THEN ? ELSE started_at END,
                completed_at = CASE WHEN ? IN ('completed','failed','cancelled') THEN ? ELSE completed_at END
            WHERE id = ?",
        )
        .bind(status.as_str())
        .bind(error_message)
        .bind(status.as_str())
        .bind(&now)
        .bind(status.as_str())
        .bind(&now)
        .bind(id.to_string())
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn reset_for_retry(&self, id: &DownloadId) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE downloads SET
                status = 'queued', error_message = NULL, retry_count = retry_count + 1,
                downloaded_size = 0, progress = 0.0, speed = 0, current_connections = 0, completed_at = NULL
            WHERE id = ?",
        )
        .bind(id.to_string())
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_priority(&self, id: &DownloadId, priority: i32) -> Result<bool> {
        let result = sqlx::query("UPDATE downloads SET priority = ? WHERE id = ?")
            .bind(priority as i64)
            .bind(id.to_string())
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_limit_rate(&self, id: &DownloadId, limit: Option<u64>) -> Result<bool> {
        let result = sqlx::query("UPDATE downloads SET limit_rate_bytes_per_sec = ? WHERE id = ?")
            .bind(limit.map(u64_to_i64))
            .bind(id.to_string())
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete(&self, id: &DownloadId) -> Result<bool> {
        let result = sqlx::query("DELETE FROM downloads WHERE id = ?")
            .bind(id.to_string())
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn counts(&self) -> Result<DownloadCounts> {
        let mut c = DownloadCounts::default();
        let row = sqlx::query(
            "SELECT COUNT(*) AS total,
                    COALESCE(SUM(downloaded_size), 0) AS bytes,
                    COALESCE(SUM(speed), 0) AS speed,
                    SUM(CASE WHEN status IN ('downloading','connecting','merging','verifying') THEN 1 ELSE 0 END) AS downloading,
                    SUM(CASE WHEN status IN ('queued','pending','retrying','waiting') THEN 1 ELSE 0 END) AS waiting,
                    SUM(CASE WHEN status = 'paused' THEN 1 ELSE 0 END) AS paused,
                    SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END) AS completed,
                    SUM(CASE WHEN status = 'failed' THEN 1 ELSE 0 END) AS failed,
                    SUM(CASE WHEN status = 'cancelled' THEN 1 ELSE 0 END) AS cancelled
             FROM downloads",
        )
        .fetch_one(self.db.pool())
        .await
        .map_err(map_err)?;
        c.total = i64_to_u64(row.try_get("total").map_err(map_err)?);
        c.total_downloaded_bytes = i64_to_u64(row.try_get("bytes").map_err(map_err)?);
        c.global_speed_bytes_per_sec = i64_to_u64(row.try_get("speed").map_err(map_err)?);
        c.downloading = i64_to_u64(
            row.try_get::<Option<i64>, _>("downloading")
                .map_err(map_err)?
                .unwrap_or(0),
        );
        c.waiting = i64_to_u64(
            row.try_get::<Option<i64>, _>("waiting")
                .map_err(map_err)?
                .unwrap_or(0),
        );
        c.paused = i64_to_u64(
            row.try_get::<Option<i64>, _>("paused")
                .map_err(map_err)?
                .unwrap_or(0),
        );
        c.completed = i64_to_u64(
            row.try_get::<Option<i64>, _>("completed")
                .map_err(map_err)?
                .unwrap_or(0),
        );
        c.failed = i64_to_u64(
            row.try_get::<Option<i64>, _>("failed")
                .map_err(map_err)?
                .unwrap_or(0),
        );
        c.cancelled = i64_to_u64(
            row.try_get::<Option<i64>, _>("cancelled")
                .map_err(map_err)?
                .unwrap_or(0),
        );
        Ok(c)
    }

    /// Replaces the segment set for a download (transactional via single statement chunks).
    pub async fn replace_segments(
        &self,
        download_id: &DownloadId,
        segments: &[SegmentInfo],
    ) -> Result<()> {
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;
        sqlx::query("DELETE FROM segments WHERE download_id = ?")
            .bind(download_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;
        for seg in segments {
            sqlx::query(
                "INSERT INTO segments(download_id, segment_index, start_offset, end_offset, downloaded_size, status, retry_count)
                 VALUES (?,?,?,?,?,?,?)",
            )
            .bind(download_id.to_string())
            .bind(seg.id as i64)
            .bind(u64_to_i64(seg.start_offset))
            .bind(u64_to_i64(seg.end_offset))
            .bind(u64_to_i64(seg.downloaded_size))
            .bind(match seg.status {
                SegmentStatus::Pending => "pending",
                SegmentStatus::Downloading => "downloading",
                SegmentStatus::Completed => "completed",
                SegmentStatus::Failed => "failed",
            })
            .bind(seg.retry_count as i64)
            .execute(&mut *tx)
            .await
            .map_err(map_err)?;
        }
        tx.commit().await.map_err(map_err)?;
        Ok(())
    }

    pub async fn load_segments(&self, download_id: &DownloadId) -> Result<Vec<SegmentInfo>> {
        let rows = sqlx::query(
            "SELECT segment_index, start_offset, end_offset, downloaded_size, status, retry_count
             FROM segments WHERE download_id = ? ORDER BY segment_index",
        )
        .bind(download_id.to_string())
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(rows
            .iter()
            .map(|row| SegmentInfo {
                id: row.get::<i64, _>("segment_index") as u32,
                start_offset: i64_to_u64(row.get("start_offset")),
                end_offset: i64_to_u64(row.get("end_offset")),
                downloaded_size: i64_to_u64(row.get("downloaded_size")),
                status: match row.get::<String, _>("status").as_str() {
                    "downloading" => SegmentStatus::Downloading,
                    "completed" => SegmentStatus::Completed,
                    "failed" => SegmentStatus::Failed,
                    _ => SegmentStatus::Pending,
                },
                retry_count: row.get::<i64, _>("retry_count") as u32,
            })
            .collect())
    }

    /// Appends a row to the events table (best-effort diagnostics).
    /// Live transfer progress: byte counts, instantaneous/average speed, progress %.
    /// Does not touch `status`, so it is safe to call while tasks are mid-flight.
    pub async fn update_progress(
        &self,
        id: &DownloadId,
        downloaded_size: u64,
        total_size: Option<u64>,
        speed: u64,
        average_speed: u64,
        progress: f64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE downloads SET
                downloaded_size = ?,
                total_size = COALESCE(?, total_size),
                speed = ?,
                average_speed = ?,
                progress = ?
             WHERE id = ?",
        )
        .bind(u64_to_i64(downloaded_size))
        .bind(total_size.map(u64_to_i64))
        .bind(u64_to_i64(speed))
        .bind(u64_to_i64(average_speed))
        .bind(progress)
        .bind(id.to_string())
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Zeroes the live-speed fields (used when a task pauses so the UI does not
    /// show a stale throughput value).
    pub async fn mark_idle(&self, id: &DownloadId) -> Result<bool> {
        let result =
            sqlx::query("UPDATE downloads SET speed = 0, current_connections = 0 WHERE id = ?")
                .bind(id.to_string())
                .execute(self.db.pool())
                .await
                .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Fills in remote metadata learned during the probe phase.
    pub async fn set_probe(
        &self,
        id: &DownloadId,
        final_url: Option<&str>,
        total_size: Option<u64>,
        etag: Option<&str>,
        last_modified: Option<&str>,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE downloads SET
                final_url = COALESCE(?, final_url),
                total_size = COALESCE(?, total_size),
                etag = COALESCE(?, etag),
                last_modified = COALESCE(?, last_modified)
             WHERE id = ?",
        )
        .bind(final_url)
        .bind(total_size.map(u64_to_i64))
        .bind(etag)
        .bind(last_modified)
        .bind(id.to_string())
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Final bookkeeping for a completed download (progress=100, counters, speed reset).
    pub async fn finish_download(
        &self,
        id: &DownloadId,
        downloaded_size: u64,
        average_speed: u64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE downloads SET
                progress = 100.0,
                downloaded_size = ?,
                average_speed = ?,
                speed = 0,
                current_connections = 0
             WHERE id = ?",
        )
        .bind(u64_to_i64(downloaded_size))
        .bind(u64_to_i64(average_speed))
        .bind(id.to_string())
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Increments the automatic-retry counter; returns the new value, or `None`
    /// when the task does not exist.
    pub async fn bump_retry(&self, id: &DownloadId) -> Result<Option<u32>> {
        let row: Option<(i64,)> = sqlx::query_as(
            "UPDATE downloads SET retry_count = retry_count + 1 WHERE id = ? RETURNING retry_count",
        )
        .bind(id.to_string())
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(row.map(|(v,)| v.clamp(0, u32::MAX as i64) as u32))
    }

    /// Backoff timer fired: flip `retrying` back to `queued`. Returns false when
    /// the task is no longer in `retrying` state (user paused/cancelled/removed it).
    pub async fn requeue_if_retrying(&self, id: &DownloadId) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE downloads SET status = 'queued' WHERE id = ? AND status = 'retrying'",
        )
        .bind(id.to_string())
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected() > 0)
    }

    /// Crash recovery, run once at daemon start: any task that was mid-flight when
    /// the process died goes back to `queued` (the `.ariaflow-part` file keeps the
    /// partially downloaded bytes and is picked up by the resume path).
    pub async fn reset_interrupted_transfers(&self) -> Result<u64> {
        let result = sqlx::query(
            "UPDATE downloads SET
                status = 'queued', speed = 0, current_connections = 0
             WHERE status IN ('connecting', 'downloading', 'merging', 'verifying', 'retrying', 'waiting')",
        )
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(result.rows_affected())
    }

    pub async fn record_event(
        &self,
        download_id: Option<&DownloadId>,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        sqlx::query("INSERT INTO events(download_id, event_type, payload) VALUES (?,?,?)")
            .bind(download_id.map(|i| i.to_string()))
            .bind(event_type)
            .bind(payload.to_string())
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        let _ = json!(payload);
        Ok(())
    }
}

pub struct HistoryRepository {
    db: Database,
}

impl HistoryRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn record(
        &self,
        d: &Download,
        result: &str,
        error_message: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO download_history(url, filename, save_path, total_size, average_speed, result, error_message, started_at, completed_at)
             VALUES (?,?,?,?,?,?,?,?,datetime('now'))",
        )
        .bind(&d.url)
        .bind(&d.filename)
        .bind(d.save_path.to_string_lossy().as_ref())
        .bind(d.total_size.map(u64_to_i64))
        .bind(u64_to_i64(d.average_speed_bytes_per_sec))
        .bind(result)
        .bind(error_message)
        .bind(d.started_at.map(|t| dt_to_str(&t)))
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(())
    }

    /// Fetch one row by primary key.
    pub async fn get(&self, id: i64) -> Result<Option<ariaflow_core::HistoryEntry>> {
        let rows = sqlx::query(
            "SELECT id, url, filename, save_path, total_size, average_speed, result, error_message, completed_at
             FROM download_history WHERE id = ?",
        )
        .bind(id)
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(rows.first().map(|row| {
            use sqlx::Row;
            ariaflow_core::HistoryEntry {
                id: row.get::<i64, _>("id"),
                url: row.get::<String, _>("url"),
                filename: row.get::<String, _>("filename"),
                save_path: row.get::<String, _>("save_path"),
                total_size: row
                    .try_get::<Option<i64>, _>("total_size")
                    .ok()
                    .flatten()
                    .map(i64_to_u64),
                average_speed: row
                    .try_get::<Option<i64>, _>("average_speed")
                    .ok()
                    .flatten()
                    .map(i64_to_u64),
                result: row.get::<String, _>("result"),
                error_message: row
                    .try_get::<Option<String>, _>("error_message")
                    .ok()
                    .flatten(),
                completed_at: row.get::<String, _>("completed_at"),
            }
        }))
    }

    /// Delete the rows with the given ids. Returns the number of rows deleted.
    pub async fn delete_ids(&self, ids: &[i64]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let mut builder = sqlx::QueryBuilder::new("DELETE FROM download_history WHERE id IN (");
        {
            let mut sep = builder.separated(", ");
            for id in ids {
                sep.push_bind(*id);
            }
        }
        builder.push(")");
        let result = builder
            .build()
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(result.rows_affected())
    }

    /// Delete every row. Returns the number of rows deleted.
    pub async fn clear(&self) -> Result<u64> {
        let result = sqlx::query("DELETE FROM download_history")
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        Ok(result.rows_affected())
    }

    pub async fn recent(&self, limit: u32) -> Result<Vec<ariaflow_core::HistoryEntry>> {
        let rows = sqlx::query(
            "SELECT id, url, filename, save_path, total_size, average_speed, result, error_message, completed_at
             FROM download_history ORDER BY completed_at DESC LIMIT ?",
        )
        .bind(limit as i64)
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(rows
            .iter()
            .map(|row| ariaflow_core::HistoryEntry {
                id: row.get::<i64, _>("id"),
                url: row.get::<String, _>("url"),
                filename: row.get::<String, _>("filename"),
                save_path: row.get::<String, _>("save_path"),
                total_size: row
                    .try_get::<Option<i64>, _>("total_size")
                    .ok()
                    .flatten()
                    .map(i64_to_u64),
                average_speed: row
                    .try_get::<Option<i64>, _>("average_speed")
                    .ok()
                    .flatten()
                    .map(i64_to_u64),
                result: row.get::<String, _>("result"),
                error_message: row
                    .try_get::<Option<String>, _>("error_message")
                    .ok()
                    .flatten(),
                completed_at: row.get::<String, _>("completed_at"),
            })
            .collect())
    }
}

pub struct SettingsRepository {
    db: Database,
}

impl SettingsRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn get_json(&self, key: &str) -> Result<Option<String>> {
        sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
            .bind(key)
            .fetch_optional(self.db.pool())
            .await
            .map_err(map_err)
    }

    pub async fn set_json(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings(key, value, updated_at) VALUES (?, ?, datetime('now'))
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = datetime('now')",
        )
        .bind(key)
        .bind(value)
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(())
    }
}

/// Repository for the `proxies` table (named, reusable proxy entries).
pub struct ProxyRepository {
    db: Database,
}

impl ProxyRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<ariaflow_core::ProxyEntry>> {
        let rows = sqlx::query(
            "SELECT id, name, kind, address, username, password, bypass, is_default, created_at
             FROM proxies ORDER BY is_default DESC, name ASC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        rows.iter().map(row_to_proxy).collect::<Result<_>>()
    }

    pub async fn insert(
        &self,
        input: &ariaflow_core::ProxyInput,
    ) -> Result<ariaflow_core::ProxyEntry> {
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;
        let id = DownloadId::new().to_string();
        if input.is_default {
            sqlx::query("UPDATE proxies SET is_default = 0")
                .execute(&mut *tx)
                .await
                .map_err(map_err)?;
        }
        sqlx::query(
            "INSERT INTO proxies(id, name, kind, address, username, password, bypass, is_default)
             VALUES (?,?,?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.kind)
        .bind(&input.address)
        .bind(&input.username)
        .bind(&input.password)
        .bind(&input.bypass)
        .bind(input.is_default as i64)
        .execute(&mut *tx)
        .await
        .map_err(map_err)?;
        tx.commit().await.map_err(map_err)?;
        Ok(ariaflow_core::ProxyEntry {
            id,
            name: input.name.clone(),
            kind: input.kind.clone(),
            address: input.address.clone(),
            username: input.username.clone(),
            password: input.password.clone(),
            bypass: input.bypass.clone(),
            is_default: input.is_default,
            created_at: None,
        })
    }

    pub async fn update(
        &self,
        id: &str,
        input: &ariaflow_core::ProxyInput,
    ) -> Result<ariaflow_core::ProxyEntry> {
        let mut tx = self.db.pool().begin().await.map_err(map_err)?;
        if input.is_default {
            sqlx::query("UPDATE proxies SET is_default = 0 WHERE id != ?")
                .bind(id)
                .execute(&mut *tx)
                .await
                .map_err(map_err)?;
        }
        let result = sqlx::query(
            "UPDATE proxies SET name = ?, kind = ?, address = ?, username = ?, password = ?,
                 bypass = ?, is_default = ?
             WHERE id = ?",
        )
        .bind(&input.name)
        .bind(&input.kind)
        .bind(&input.address)
        .bind(&input.username)
        .bind(&input.password)
        .bind(&input.bypass)
        .bind(input.is_default as i64)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(map_err)?;
        if result.rows_affected() == 0 {
            return Err(AriaFlowError::NotFound(id.to_string()));
        }
        tx.commit().await.map_err(map_err)?;
        Ok(ariaflow_core::ProxyEntry {
            id: id.to_string(),
            name: input.name.clone(),
            kind: input.kind.clone(),
            address: input.address.clone(),
            username: input.username.clone(),
            password: input.password.clone(),
            bypass: input.bypass.clone(),
            is_default: input.is_default,
            created_at: None,
        })
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM proxies WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        if result.rows_affected() == 0 {
            return Err(AriaFlowError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

fn row_to_proxy(row: &sqlx::sqlite::SqliteRow) -> Result<ariaflow_core::ProxyEntry> {
    Ok(ariaflow_core::ProxyEntry {
        id: row.try_get("id").map_err(map_err)?,
        name: row.try_get("name").map_err(map_err)?,
        kind: row.try_get("kind").map_err(map_err)?,
        address: row.try_get("address").map_err(map_err)?,
        username: row.try_get("username").map_err(map_err)?,
        password: row.try_get("password").map_err(map_err)?,
        bypass: row.try_get("bypass").map_err(map_err)?,
        is_default: row.try_get::<i64, _>("is_default").map_err(map_err)? != 0,
        created_at: row.try_get("created_at").map_err(map_err)?,
    })
}
/// Repository for the `ssh_certificates` table (SSH private keys used for
/// ssh/sftp certificate authentication).
///
/// Key material lives only in this table and in a short-lived
/// `ProviderOptions::ssh_certificate` while a transfer runs. It is never
/// returned to API callers, never logged, never serialized into a `Download`.
pub struct SshCertificateRepository {
    db: Database,
}

/// Parsed + validated private key (fingerprint computed for display).
struct ParsedSshKey {
    algorithm: String,
    fingerprint: String,
    /// True when the PEM itself was encrypted (and we hold the passphrase).
    _encrypted: bool,
}

fn parse_private_key(pem: &str, passphrase: Option<&str>) -> Result<ParsedSshKey> {
    let pem = pem.trim();
    if pem.is_empty() {
        return Err(AriaFlowError::InvalidRequest(
            "private key must not be empty".to_string(),
        ));
    }
    let key = ssh_key::PrivateKey::from_openssh(pem)
        .map_err(|e| AriaFlowError::InvalidRequest(format!("invalid OpenSSH private key: {e}")))?;
    let (resolved, encrypted) = if key.is_encrypted() {
        let passphrase = passphrase.ok_or_else(|| {
            AriaFlowError::InvalidRequest(
                "private key is encrypted; a passphrase is required".to_string(),
            )
        })?;
        let decrypted = key.decrypt(passphrase.as_bytes()).map_err(|_| {
            AriaFlowError::InvalidRequest(
                "cannot decrypt private key (wrong passphrase?)".to_string(),
            )
        })?;
        (decrypted, true)
    } else {
        (key, false)
    };
    let algorithm = resolved.algorithm().to_string();
    let fingerprint = resolved
        .public_key()
        .fingerprint(ssh_key::HashAlg::Sha256)
        .to_string();
    Ok(ParsedSshKey {
        algorithm,
        fingerprint,
        _encrypted: encrypted,
    })
}

impl SshCertificateRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    /// Lists certificate metadata (no key material) — `name` ASC, id tiebreak.
    pub async fn list(&self) -> Result<Vec<ariaflow_core::SshCertificateEntry>> {
        let rows = sqlx::query(
            "SELECT id, name, algorithm, fingerprint, (passphrase IS NOT NULL) AS has_passphrase, created_at
             FROM ssh_certificates ORDER BY name ASC, id ASC",
        )
        .fetch_all(self.db.pool())
        .await
        .map_err(map_err)?;
        rows.iter().map(row_to_certificate).collect()
    }

    /// (name, private_key, passphrase) for a single certificate — the
    /// engine's auth material. Never exposed through any API surface.
    pub async fn material(&self, id: &str) -> Result<Option<(String, String, Option<String>)>> {
        let row: Option<(String, String, Option<String>)> = sqlx::query_as(
            "SELECT name, private_key, passphrase FROM ssh_certificates WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(row)
    }

    pub async fn insert(
        &self,
        input: &ariaflow_core::SshCertificateInput,
    ) -> Result<ariaflow_core::SshCertificateEntry> {
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(AriaFlowError::InvalidRequest(
                "certificate name must not be empty".to_string(),
            ));
        }
        let pem = input
            .private_key
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .ok_or_else(|| {
                AriaFlowError::InvalidRequest("a private key is required".to_string())
            })?;
        let passphrase = input.passphrase.as_deref().filter(|p| !p.is_empty());
        let parsed = parse_private_key(pem, passphrase)?;
        // Unencrypted keys never store a passphrase.
        let stored_passphrase = if parsed._encrypted { passphrase } else { None };
        let id = DownloadId::new().to_string();
        sqlx::query(
            "INSERT INTO ssh_certificates(id, name, private_key, passphrase, algorithm, fingerprint)
             VALUES (?,?,?,?,?,?)",
        )
        .bind(&id)
        .bind(&name)
        .bind(pem)
        .bind(stored_passphrase)
        .bind(&parsed.algorithm)
        .bind(&parsed.fingerprint)
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(ariaflow_core::SshCertificateEntry {
            id,
            name,
            algorithm: parsed.algorithm,
            fingerprint: parsed.fingerprint,
            has_passphrase: stored_passphrase.is_some(),
            created_at: None,
        })
    }

    pub async fn update(
        &self,
        id: &str,
        input: &ariaflow_core::SshCertificateInput,
    ) -> Result<ariaflow_core::SshCertificateEntry> {
        let existing: Option<(String, Option<String>, String, String)> = sqlx::query_as(
            "SELECT private_key, passphrase, algorithm, fingerprint FROM ssh_certificates WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(self.db.pool())
        .await
        .map_err(map_err)?;
        let (existing_key, existing_passphrase, existing_algorithm, existing_fingerprint) =
            existing.ok_or_else(|| AriaFlowError::NotFound(id.to_string()))?;

        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(AriaFlowError::InvalidRequest(
                "certificate name must not be empty".to_string(),
            ));
        }

        let new_pem = input
            .private_key
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string);
        let passphrase_patch = input.passphrase.as_deref();

        let (pem, stored_passphrase, algorithm, fingerprint) = if let Some(pem) = new_pem {
            // New key material: validate it on its own terms; any previous
            // passphrase is discarded.
            let passphrase = passphrase_patch.filter(|p| !p.is_empty());
            let parsed = parse_private_key(&pem, passphrase)?;
            let stored = if parsed._encrypted { passphrase } else { None };
            (
                pem,
                stored.map(str::to_string),
                parsed.algorithm,
                parsed.fingerprint,
            )
        } else {
            // Keep the stored key; only the passphrase may change.
            let stored_passphrase = match passphrase_patch {
                None => existing_passphrase.clone(),
                Some("") => {
                    // Refusing to strand an encrypted key without its passphrase.
                    parse_private_key(&existing_key, None)
                        .map(|_| ())
                        .map_err(|_| {
                            AriaFlowError::InvalidRequest(
                                "stored key is encrypted; a passphrase is required".to_string(),
                            )
                        })?;
                    None
                }
                Some(pass) => {
                    parse_private_key(&existing_key, Some(pass))?;
                    let encrypted = ssh_key::PrivateKey::from_openssh(&existing_key)
                        .map(|k| k.is_encrypted())
                        .unwrap_or(false);
                    if encrypted {
                        Some(pass.to_string())
                    } else {
                        None
                    }
                }
            };
            (
                existing_key,
                stored_passphrase,
                existing_algorithm,
                existing_fingerprint,
            )
        };

        sqlx::query(
            "UPDATE ssh_certificates
             SET name = ?, private_key = ?, passphrase = ?, algorithm = ?, fingerprint = ?
             WHERE id = ?",
        )
        .bind(&name)
        .bind(&pem)
        .bind(&stored_passphrase)
        .bind(&algorithm)
        .bind(&fingerprint)
        .bind(id)
        .execute(self.db.pool())
        .await
        .map_err(map_err)?;
        Ok(ariaflow_core::SshCertificateEntry {
            id: id.to_string(),
            name,
            algorithm,
            fingerprint,
            has_passphrase: stored_passphrase.is_some(),
            created_at: None,
        })
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let result = sqlx::query("DELETE FROM ssh_certificates WHERE id = ?")
            .bind(id)
            .execute(self.db.pool())
            .await
            .map_err(map_err)?;
        if result.rows_affected() == 0 {
            return Err(AriaFlowError::NotFound(id.to_string()));
        }
        Ok(())
    }
}

fn row_to_certificate(row: &sqlx::sqlite::SqliteRow) -> Result<ariaflow_core::SshCertificateEntry> {
    Ok(ariaflow_core::SshCertificateEntry {
        id: row.try_get("id").map_err(map_err)?,
        name: row.try_get("name").map_err(map_err)?,
        algorithm: row.try_get("algorithm").map_err(map_err)?,
        fingerprint: row.try_get("fingerprint").map_err(map_err)?,
        has_passphrase: row.try_get::<i64, _>("has_passphrase").map_err(map_err)? != 0,
        created_at: row.try_get("created_at").map_err(map_err)?,
    })
}
