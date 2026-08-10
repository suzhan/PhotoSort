//! Planner：把 (PhotoFile, PhotoMetadata) + 设置 → PhotoPlan。
//!
//! 红线（需求 §四十六）：Planner 只读文件系统（exists 探测），绝不修改任何文件。
//! Preview 与正式执行必须走同一个 Planner，保证「所见即所执行」。

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing::warn;

use super::duplicate::DestinationIndex;
use super::sequence::SequenceCoordinator;
use super::template::{render_directory, render_filename, Template, TemplateContext};
use crate::error::{AppError, Result};
use crate::models::duplicate::DuplicateResult;
use crate::models::metadata::{
    GpsCoordinate, LocationSource, PhotoMetadata, ResolvedLocation, TakenAtSource,
};
use crate::models::photo::PhotoFile;
use crate::models::plan::{PhotoPlan, PlanStatus, PlanWarning};
use crate::models::settings::{AppSettings, GpsNoApiMode};
use crate::utils::path::ensure_within;

/// 序号重试上限：防撞死循环的保险丝，正常永远触不到。
const MAX_SEQ_ATTEMPTS: u32 = 100_000;

/// 目标占用探测：真实运行用文件系统，测试用内存集合。
pub type ExistsProbe = Arc<dyn Fn(&Path) -> bool + Send + Sync>;

pub fn fs_probe() -> ExistsProbe {
    Arc::new(|p: &Path| p.exists())
}

pub struct Planner {
    /// 复制持有而非借用：Planner 会随 spawn_blocking 进入后台线程（'static）。
    settings: AppSettings,
    dir_template: Template,
    file_template: Template,
    sequences: SequenceCoordinator,
    /// 本次运行已分配的目标路径：防止同批两张照片规划到同一文件。
    reserved: Mutex<HashSet<PathBuf>>,
    probe: ExistsProbe,
    /// 目标目录内容索引：Collision 裁决与跨目录 Duplicate 检测。
    dedupe: Option<DestinationIndex>,
}

impl Planner {
    pub fn new(settings: &AppSettings) -> Result<Self> {
        Self::with_probe(settings, fs_probe())
    }

    pub fn with_probe(settings: &AppSettings, probe: ExistsProbe) -> Result<Self> {
        Ok(Self {
            settings: settings.clone(),
            dir_template: Template::validate_directory(&settings.directory_template)?,
            file_template: Template::validate_filename(&settings.filename_template)?,
            sequences: SequenceCoordinator::new(),
            reserved: Mutex::new(HashSet::new()),
            probe,
            dedupe: None,
        })
    }

    /// 挂上目标目录索引后，Planner 具备内容级查重能力。
    pub fn with_index(mut self, index: DestinationIndex) -> Self {
        self.dedupe = Some(index);
        self
    }

    pub fn destination_root(&self) -> Result<PathBuf> {
        self.settings
            .destination_directory
            .clone()
            .ok_or_else(|| AppError::Config("destination directory not set".to_string()))
    }

    /// 生成单张照片的 Plan。永不 panic / 永不修改文件；
    /// 可恢复错误折叠为 status = Error。
    pub fn plan(&self, photo: &PhotoFile, metadata: Option<&PhotoMetadata>) -> PhotoPlan {
        self.plan_with_location(photo, metadata, None)
    }

    /// 带预解析 location 的 plan：GPS 反查由调用方在流水线中完成后注入。
    /// 注入值优先于 Planner 自身的坐标/未知降级。
    pub fn plan_with_location(
        &self,
        photo: &PhotoFile,
        metadata: Option<&PhotoMetadata>,
        pre_resolved: Option<&ResolvedLocation>,
    ) -> PhotoPlan {
        match self.plan_inner(photo, metadata, pre_resolved) {
            Ok(plan) => plan,
            Err(e) => {
                warn!(
                    path = %photo.path.to_string_lossy(),
                    error = %e,
                    "planning failed"
                );
                PhotoPlan {
                    source: photo.clone(),
                    metadata: metadata.cloned(),
                    location: None,
                    target_path: photo.path.clone(),
                    status: PlanStatus::Error,
                    duplicate: DuplicateResult::NotDuplicate,
                    warnings: vec![],
                }
            }
        }
    }

    fn plan_inner(
        &self,
        photo: &PhotoFile,
        metadata: Option<&PhotoMetadata>,
        pre_resolved: Option<&ResolvedLocation>,
    ) -> Result<PhotoPlan> {
        let dest_root = self.destination_root()?;
        let location = pre_resolved
            .cloned()
            .or_else(|| resolve_location(&self.settings, metadata.and_then(|m| m.gps)));
        let mut ctx = TemplateContext::from_parts(
            photo,
            metadata,
            location.clone(),
            self.settings.metadata_fallback.clone(),
            None,
        );

        let components = render_directory(&self.dir_template, &ctx)?;
        let mut target_dir = dest_root.clone();
        for c in &components {
            target_dir.push(c);
        }
        ensure_within(&dest_root, &target_dir).map_err(|e| {
            AppError::InvalidPath(format!("target dir escapes destination root: {e}"))
        })?;

        let uses_seq = self.file_template.uses_seq();
        let mut attempts: u32 = 0;
        let (target_path, collision) = loop {
            if uses_seq {
                ctx.seq = Some(self.sequences.next(&target_dir));
            }
            let filename = render_filename(&self.file_template, &ctx)?;
            let candidate = target_dir.join(&filename);
            ensure_within(&dest_root, &candidate).map_err(|e| {
                AppError::InvalidPath(format!("target file escapes destination root: {e}"))
            })?;
            if !self.is_taken(&candidate) {
                break (candidate, false);
            }
            if !uses_seq {
                // 无序号模板无法自动避让，冲突交给 P7 内容查重裁决
                break (candidate, true);
            }
            attempts += 1;
            if attempts >= MAX_SEQ_ATTEMPTS {
                return Err(AppError::Task(format!(
                    "seq allocation exhausted in {}",
                    target_dir.to_string_lossy()
                )));
            }
        };
        self.reserve(&target_path);

        // 内容级查重（需求 §十三）：Collision 可能是 Duplicate；空闲目标也可能
        // 在目标树别处已有同内容文件。哈希失败按保守策略处理（不判等、不删源）。
        let mut duplicate = DuplicateResult::NotDuplicate;
        if let Some(index) = &self.dedupe {
            if collision {
                match index.equals_file(&photo.path, &target_path) {
                    Ok(true) => {
                        duplicate = DuplicateResult::ExactDuplicate {
                            existing_path: target_path.clone(),
                        };
                    }
                    Ok(false) => {}
                    Err(e) => {
                        warn!(path = %photo.path.to_string_lossy(), error = %e,
                              "collision content check failed; keeping Collision");
                    }
                }
            } else {
                match index.find_duplicate(&photo.path, photo.size) {
                    Ok(Some(existing)) => {
                        duplicate = DuplicateResult::ExactDuplicate {
                            existing_path: existing,
                        };
                    }
                    Ok(None) => {}
                    Err(e) => {
                        warn!(path = %photo.path.to_string_lossy(), error = %e,
                              "duplicate search failed; treating as not duplicate");
                    }
                }
            }
        }

        let status = if duplicate.is_duplicate() {
            PlanStatus::Duplicate
        } else if collision {
            PlanStatus::Collision
        } else {
            self.classify(metadata, &location)
        };

        let mut warnings = Vec::new();
        if metadata.and_then(|m| m.taken_at_source) == Some(TakenAtSource::FileModifiedFallback) {
            warnings.push(PlanWarning::UsedModifiedTimeFallback);
        }
        if metadata.and_then(|m| m.camera_model.as_ref()).is_none() {
            warnings.push(PlanWarning::MissingCameraModel);
        }
        if metadata.and_then(|m| m.lens_model.as_ref()).is_none() {
            warnings.push(PlanWarning::MissingLensModel);
        }
        if location.is_none() {
            warnings.push(PlanWarning::MissingGps);
        }

        Ok(PhotoPlan {
            source: photo.clone(),
            metadata: metadata.cloned(),
            location,
            target_path,
            status,
            duplicate,
            warnings,
        })
    }

    /// 非冲突情况下的状态分级：缺失信息不阻塞（fallback 名称兜底），
    /// 但必须在预览里明示。
    fn classify(
        &self,
        metadata: Option<&PhotoMetadata>,
        location: &Option<ResolvedLocation>,
    ) -> PlanStatus {
        let wants_metadata =
            self.dir_template.uses_metadata() || self.file_template.uses_metadata();
        if !wants_metadata {
            return PlanStatus::Ready;
        }

        let uses_date = self.dir_template.uses_date() || self.file_template.uses_date();
        if uses_date && metadata.and_then(|m| m.taken_at).is_none() {
            return PlanStatus::MissingDate;
        }

        let uses_gps = self.dir_template.uses_gps() || self.file_template.uses_gps();
        if uses_gps && location.is_none() && !self.gps_intentionally_dropped(metadata) {
            return PlanStatus::MissingGps;
        }

        let empty = metadata.is_none_or(|m| {
            m.taken_at.is_none()
                && m.camera_make.is_none()
                && m.camera_model.is_none()
                && m.lens_make.is_none()
                && m.lens_model.is_none()
                && m.gps.is_none()
        });
        if empty {
            return PlanStatus::MissingExif;
        }
        PlanStatus::Ready
    }

    /// 用户显式选择 Ignore / UnknownLocation 时，GPS 缺席是意愿而非异常。
    fn gps_intentionally_dropped(&self, metadata: Option<&PhotoMetadata>) -> bool {
        self.settings.gps_enabled
            && metadata.and_then(|m| m.gps).is_some()
            && matches!(
                self.settings.gps_no_api_mode,
                GpsNoApiMode::Ignore | GpsNoApiMode::UnknownLocation
            )
    }

    fn is_taken(&self, candidate: &Path) -> bool {
        let reserved = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
        reserved.contains(candidate) || (self.probe)(candidate)
    }

    fn reserve(&self, target: &Path) {
        let mut reserved = self.reserved.lock().unwrap_or_else(|e| e.into_inner());
        reserved.insert(target.to_path_buf());
    }
}

/// GPS 位置解析（预览期）：无 Google API（P12 才接入），按设置的退化策略处理。
pub fn resolve_location(
    settings: &AppSettings,
    gps: Option<GpsCoordinate>,
) -> Option<ResolvedLocation> {
    if !settings.gps_enabled {
        return None;
    }
    let gps = gps?;
    match settings.gps_no_api_mode {
        GpsNoApiMode::Ignore | GpsNoApiMode::UnknownLocation => None,
        GpsNoApiMode::Coordinates => {
            let p = settings.gps_round_precision as usize;
            Some(ResolvedLocation {
                country: None,
                province: None,
                city: Some(format!("{:.p$}_{:.p$}", gps.latitude, gps.longitude)),
                district: None,
                formatted_address: None,
                source: LocationSource::Coordinates,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
    use std::time::SystemTime;

    fn photo(name: &str) -> PhotoFile {
        PhotoFile {
            path: PathBuf::from(format!("/src/{name}")),
            size: 1024,
            extension: "jpg".to_string(),
            modified_time: SystemTime::UNIX_EPOCH,
        }
    }

    fn metadata() -> PhotoMetadata {
        PhotoMetadata {
            taken_at: Some(NaiveDateTime::new(
                NaiveDate::from_ymd_opt(2017, 11, 30).expect("d"),
                NaiveTime::from_hms_opt(15, 22, 31).expect("t"),
            )),
            camera_model: Some("NIKON D80".to_string()),
            gps: Some(GpsCoordinate {
                latitude: 22.319345,
                longitude: 114.169423,
            }),
            ..Default::default()
        }
    }

    fn settings(dest: &str) -> AppSettings {
        AppSettings {
            destination_directory: Some(PathBuf::from(dest)),
            ..Default::default()
        }
    }

    fn planner_with(s: &AppSettings, existing: &[&str]) -> Planner {
        let set: HashSet<PathBuf> = existing.iter().map(PathBuf::from).collect();
        Planner::with_probe(s, Arc::new(move |p: &Path| set.contains(p))).expect("planner")
    }

    #[test]
    fn plans_default_templates() {
        let s = settings("/dest");
        let p = planner_with(&s, &[]);
        let plan = p.plan(&photo("DSC_1231.JPG"), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Ready);
        assert_eq!(
            plan.target_path,
            PathBuf::from("/dest/2017/NIKON D80/DSC_1231.JPG")
        );
    }

    #[test]
    fn missing_date_is_status_not_error() {
        let s = settings("/dest");
        let p = planner_with(&s, &[]);
        let mut md = metadata();
        md.taken_at = None;
        let plan = p.plan(&photo("a.jpg"), Some(&md));
        assert_eq!(plan.status, PlanStatus::MissingDate);
        assert_eq!(
            plan.target_path,
            PathBuf::from("/dest/UnknownDate/NIKON D80/a.jpg")
        );
        assert!(plan.executable());
    }

    #[test]
    fn gps_coordinates_fallback_rounded() {
        let mut s = settings("/dest");
        s.gps_enabled = true;
        s.directory_template = "{yyyy}/{gps_city}".to_string();
        let p = planner_with(&s, &[]);
        let plan = p.plan(&photo("a.jpg"), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Ready);
        assert_eq!(
            plan.target_path,
            PathBuf::from("/dest/2017/22.3193_114.1694/a.jpg")
        );
        let loc = plan.location.expect("location");
        assert_eq!(loc.source, LocationSource::Coordinates);
    }

    #[test]
    fn gps_ignore_mode_suppresses_missing_gps() {
        let mut s = settings("/dest");
        s.gps_enabled = true;
        s.gps_no_api_mode = GpsNoApiMode::Ignore;
        s.directory_template = "{yyyy}/{gps_city}".to_string();
        let p = planner_with(&s, &[]);
        let plan = p.plan(&photo("a.jpg"), Some(&metadata()));
        // Ignore：GPS 不进路径（UnknownLocation 兜底），但也不报警
        assert_eq!(plan.status, PlanStatus::Ready);
        assert!(plan.target_path.starts_with("/dest/2017/UnknownLocation"));
    }

    #[test]
    fn gps_disabled_with_gps_template_flags_missing() {
        let mut s = settings("/dest");
        s.directory_template = "{yyyy}/{gps_city}".to_string();
        s.gps_enabled = false;
        let p = planner_with(&s, &[]);
        let plan = p.plan(&photo("a.jpg"), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::MissingGps);
    }

    #[test]
    fn collision_without_seq_template() {
        let s = settings("/dest");
        let existing = ["/dest/2017/NIKON D80/a.jpg"];
        let p = planner_with(&s, &existing);
        let plan = p.plan(&photo("a.jpg"), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Collision);
        assert!(!plan.executable());
    }

    #[test]
    fn seq_template_auto_avoids_collision() {
        let mut s = settings("/dest");
        s.filename_template = "{yyyyMMdd}_{seq:4}.{extension}".to_string();
        let existing = ["/dest/2017/NIKON D80/20171130_0001.jpg"];
        let p = planner_with(&s, &existing);
        let plan = p.plan(&photo("a.jpg"), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Ready);
        assert_eq!(
            plan.target_path,
            PathBuf::from("/dest/2017/NIKON D80/20171130_0002.jpg")
        );
    }

    #[test]
    fn two_identical_photos_get_distinct_targets_in_run() {
        let mut s = settings("/dest");
        s.filename_template = "{yyyyMMdd}_{seq:2}.{extension}".to_string();
        let p = planner_with(&s, &[]);
        let p1 = p.plan(&photo("a.jpg"), Some(&metadata()));
        let p2 = p.plan(&photo("b.jpg"), Some(&metadata()));
        assert_ne!(p1.target_path, p2.target_path);
        assert!(p1.target_path.to_string_lossy().ends_with("_01.jpg"));
        assert!(p2.target_path.to_string_lossy().ends_with("_02.jpg"));
    }

    #[test]
    fn in_run_reservation_blocks_second_identical_name() {
        let s = settings("/dest");
        let p = planner_with(&s, &[]);
        let p1 = p.plan(&photo("a.jpg"), Some(&metadata()));
        let p2 = p.plan(&photo("a.jpg"), Some(&metadata())); // 不同源目录同名文件
        assert_eq!(p1.status, PlanStatus::Ready);
        assert_eq!(p2.status, PlanStatus::Collision);
    }

    #[test]
    fn modified_time_fallback_is_audited() {
        let s = settings("/dest");
        let p = planner_with(&s, &[]);
        let mut md = metadata();
        md.taken_at_source = Some(TakenAtSource::FileModifiedFallback);
        let plan = p.plan(&photo("a.jpg"), Some(&md));
        assert!(plan
            .warnings
            .contains(&PlanWarning::UsedModifiedTimeFallback));
    }

    #[test]
    fn no_metadata_template_always_ready() {
        let mut s = settings("/dest");
        s.directory_template = "by_source".to_string();
        s.filename_template = "{original_name}.{extension}".to_string();
        let p = planner_with(&s, &[]);
        let plan = p.plan(&photo("a.jpg"), None);
        assert_eq!(plan.status, PlanStatus::Ready);
        assert_eq!(plan.target_path, PathBuf::from("/dest/by_source/a.jpg"));
    }

    #[test]
    fn escape_attempt_in_template_is_contained() {
        let mut s = settings("/dest");
        s.directory_template = "../../etc".to_string();
        let p = planner_with(&s, &[]);
        let plan = p.plan(&photo("a.jpg"), Some(&metadata()));
        // 清洗后落在 dest 内，且 ensure_within 复核通过
        assert_ne!(plan.status, PlanStatus::Error);
        assert!(plan.target_path.starts_with("/dest"));
    }

    #[test]
    fn missing_destination_is_config_error() {
        let s = AppSettings::default();
        let p = planner_with(&s, &[]);
        let plan = p.plan(&photo("a.jpg"), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Error);
    }

    // ---- 内容级查重（真实临时文件，走 fs probe + 真索引）----

    fn real_photo(path: &Path) -> PhotoFile {
        PhotoFile {
            path: path.to_path_buf(),
            size: std::fs::metadata(path).expect("stat").len(),
            extension: "jpg".to_string(),
            modified_time: SystemTime::UNIX_EPOCH,
        }
    }

    #[test]
    fn collision_with_identical_content_becomes_duplicate() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(dest.join("2017/NIKON D80")).expect("mkdir");
        std::fs::write(dest.join("2017/NIKON D80/a.jpg"), b"same-bytes").expect("w");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        let source = src.join("a.jpg");
        std::fs::write(&source, b"same-bytes").expect("w");

        let mut s = settings(dest.to_str().expect("utf8"));
        s.source_directory = Some(src.clone());
        let index = DestinationIndex::build(&dest, s.duplicate_mode).expect("index");
        let p = Planner::new(&s).expect("planner").with_index(index);

        let plan = p.plan(&real_photo(&source), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Duplicate);
        assert!(plan.duplicate.is_duplicate());
        assert!(plan.executable()); // Duplicate 可执行：执行器将跳过复制
    }

    #[test]
    fn collision_with_different_content_stays_collision() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(dest.join("2017/NIKON D80")).expect("mkdir");
        std::fs::write(dest.join("2017/NIKON D80/a.jpg"), b"other-bytes!").expect("w");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        let source = src.join("a.jpg");
        std::fs::write(&source, b"same-length?").expect("w"); // 同尺寸异内容

        let s = settings(dest.to_str().expect("utf8"));
        let index = DestinationIndex::build(&dest, s.duplicate_mode).expect("index");
        let p = Planner::new(&s).expect("planner").with_index(index);

        let plan = p.plan(&real_photo(&source), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Collision);
        assert!(!plan.duplicate.is_duplicate());
        assert!(!plan.executable());
    }

    #[test]
    fn duplicate_found_elsewhere_in_archive_tree() {
        let tmp = tempfile::tempdir().expect("tmp");
        let dest = tmp.path().join("dest");
        std::fs::create_dir_all(dest.join("2016/Canon")).expect("mkdir");
        let archived = dest.join("2016/Canon/already.jpg");
        std::fs::write(&archived, b"photo-bytes").expect("w");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        let source = src.join("new.jpg");
        std::fs::write(&source, b"photo-bytes").expect("w");

        let s = settings(dest.to_str().expect("utf8"));
        let index = DestinationIndex::build(&dest, s.duplicate_mode).expect("index");
        let p = Planner::new(&s).expect("planner").with_index(index);

        let plan = p.plan(&real_photo(&source), Some(&metadata()));
        assert_eq!(plan.status, PlanStatus::Duplicate);
        match plan.duplicate {
            DuplicateResult::ExactDuplicate { existing_path } => {
                assert_eq!(existing_path, archived);
            }
            _ => panic!("expected ExactDuplicate"),
        }
    }
}
