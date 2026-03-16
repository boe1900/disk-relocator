<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { computed, onBeforeUnmount, ref, watch } from "vue";
import { AlertCircle, ArrowRightLeft, CheckCircle2, HardDrive } from "lucide-vue-next";
import type {
  AppScanPath,
  AppScanResult,
  DiskStatus,
  MigrateRequest,
  RelocationResult
} from "../types/contracts";
import { useI18n } from "../i18n";
import { formatCommandError } from "../utils/error";

interface SourceSummary {
  exists: boolean;
  hasSymlink: boolean;
  sizeBytes: number;
  hasData: boolean;
  path: string;
  unitId: string | null;
  unitLabel: string;
  enabled: boolean;
  riskLevel: string;
  requiresConfirmation: boolean;
  blockedReason: string | null;
  allowBootstrapIfSourceMissing: boolean;
}

interface MigrationPlanItem {
  key: string;
  app: AppScanResult;
  unitId: string | null;
  unitLabel: string;
  sourcePath: string;
  sourceSizeBytes: number;
  mode: MigrateRequest["mode"];
  alreadyMigrated: boolean;
  execute: boolean;
  reason: string;
  requiresConfirmation: boolean;
}

const props = defineProps<{
  showModal: boolean;
  selectedAppId: string;
  selectedApp: AppScanResult | null;
  disks: DiskStatus[];
}>();

const emit = defineEmits<{
  (e: "close"): void;
  (e: "done", message: string): void;
}>();
const { t, locale } = useI18n();

const migrationStep = ref(0); // 0: setup, 1: migrating, 2: success
const progress = ref(0);
const targetDiskMount = ref("");
const targetRoot = ref("");
const confirmHighRisk = ref(false);
const cleanupBackupAfterMigrate = ref(true);
const loading = ref(false);
const selectingTargetRoot = ref(false);
const error = ref<string | null>(null);
const successResults = ref<RelocationResult[]>([]);
const copiedPathKey = ref<string | null>(null);
const activePlanGroupKey = ref("");
const showRiskWarningModal = ref(false);
const warningCountdownRemaining = ref(0);

let progressTimer: number | null = null;
let copyFeedbackTimer: number | null = null;
let warningCountdownTimer: number | null = null;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null;
}

function parseJsonIfPossible(input: string): unknown {
  const trimmed = input.trim();
  if (!trimmed.startsWith("{") && !trimmed.startsWith("[")) {
    return input;
  }
  try {
    return JSON.parse(trimmed);
  } catch {
    return input;
  }
}

function normalizeInvokeError(err: unknown): unknown {
  if (typeof err === "string") {
    return parseJsonIfPossible(err);
  }
  return err;
}

function formatMigrationStartError(err: unknown): string {
  const normalized = normalizeInvokeError(err);
  if (!isRecord(normalized)) {
    return t("migrationDialog.errors.startFailed", { error: formatCommandError(err) });
  }

  const code = typeof normalized.code === "string" ? normalized.code : "";
  const traceId = typeof normalized.trace_id === "string" ? normalized.trace_id : "";
  const details = isRecord(normalized.details) ? normalized.details : null;
  const withTrace = (message: string): string =>
    traceId ? `${message} (trace_id=${traceId})` : message;

  if (code === "PRECHECK_TARGET_PATH_EXISTS") {
    const targetPath =
      details && typeof details.target_path === "string" ? details.target_path.trim() : "";
    if (targetPath) {
      return withTrace(
        t("migrationDialog.errors.targetPathExists", {
          path: targetPath
        })
      );
    }
    return withTrace(t("migrationDialog.errors.targetPathExistsNoPath"));
  }

  if (code === "PRECHECK_BACKUP_PATH_EXISTS") {
    const backupPath =
      details && typeof details.backup_path === "string" ? details.backup_path.trim() : "";
    if (backupPath) {
      return withTrace(
        t("migrationDialog.errors.backupPathExists", {
          path: backupPath
        })
      );
    }
    return withTrace(t("migrationDialog.errors.backupPathExistsNoPath"));
  }

  return t("migrationDialog.errors.startFailed", { error: formatCommandError(err) });
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 1)} ${units[unitIndex]}`;
}

function createBatchTraceId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return `tr_batch_${crypto.randomUUID()}`;
  }
  return `tr_batch_${Date.now()}_${Math.random().toString(36).slice(2, 10)}`;
}

function normalizeDetectedPaths(app: AppScanResult): AppScanPath[] {
  const paths = app.detected_paths ?? [];
  if (paths.length > 0) {
    return paths;
  }
  return [
    {
      display_name: app.display_name,
      default_enabled: true,
      path: "",
      exists: false,
      is_symlink: false,
      size_bytes: 0
    }
  ];
}

function resolveUnitLabel(app: AppScanResult, path: AppScanPath, index: number): string {
  if (path.display_name?.trim()) {
    return path.display_name.trim();
  }
  if (path.unit_id?.trim()) {
    return path.unit_id.trim();
  }
  if (path.path.trim()) {
    const segments = path.path.split("/").filter(Boolean);
    const tail = segments[segments.length - 1];
    if (tail) {
      return tail;
    }
  }
  return `${app.display_name} #${index + 1}`;
}

function summarizeSource(app: AppScanResult, path: AppScanPath, index: number): SourceSummary {
  const sizeBytes = path.exists && !path.is_symlink ? Math.max(0, path.size_bytes) : 0;
  const riskLevel = (path.risk_level ?? "stable").toString();
  const enabled = path.enabled !== false;
  const blockedReason = path.blocked_reason?.trim() || null;
  return {
    exists: path.exists,
    hasSymlink: path.exists && path.is_symlink,
    sizeBytes,
    hasData: sizeBytes > 0,
    path: path.path ?? "",
    unitId: path.unit_id?.trim() || null,
    unitLabel: resolveUnitLabel(app, path, index),
    enabled,
    riskLevel,
    requiresConfirmation: riskLevel.toLowerCase() === "high",
    blockedReason,
    allowBootstrapIfSourceMissing: path.allow_bootstrap_if_source_missing === true
  };
}

function pickMode(source: SourceSummary): MigrateRequest["mode"] {
  if (source.hasData) {
    return "migrate";
  }
  if (source.allowBootstrapIfSourceMissing) {
    return "bootstrap";
  }
  return "migrate";
}

function buildPlanItem(app: AppScanResult, path: AppScanPath, index: number): MigrationPlanItem {
  const source = summarizeSource(app, path, index);
  const mode = pickMode(source);
  const key = source.unitId ? source.unitId : `path-${index + 1}`;
  const base: Omit<MigrationPlanItem, "execute" | "reason"> = {
    key,
    app,
    unitId: source.unitId,
    unitLabel: source.unitLabel,
    sourcePath: source.path,
    sourceSizeBytes: source.sizeBytes,
    mode,
    alreadyMigrated: false,
    requiresConfirmation: source.requiresConfirmation
  };

  if (app.availability === "blocked" || app.availability === "deprecated") {
    return { ...base, execute: false, reason: t("migrationDialog.reason.blocked") };
  }
  if (app.running) {
    return { ...base, execute: false, reason: t("migrationDialog.reason.running") };
  }
  if (!source.enabled) {
    return { ...base, execute: false, reason: source.blockedReason || t("migrationDialog.reason.blocked") };
  }
  if (source.blockedReason) {
    return { ...base, execute: false, reason: source.blockedReason };
  }
  if (source.exists && source.hasSymlink) {
    return { ...base, alreadyMigrated: true, execute: false, reason: t("migrationDialog.reason.migrated") };
  }
  if (!source.exists && !source.allowBootstrapIfSourceMissing) {
    return { ...base, execute: false, reason: t("migrationDialog.reason.sourceMissingNoData") };
  }
  if (!source.exists && mode === "bootstrap") {
    return { ...base, execute: true, reason: t("migrationDialog.reason.sourceMissingBootstrap") };
  }
  if (!source.hasData && mode === "bootstrap") {
    return { ...base, execute: true, reason: t("migrationDialog.reason.sourceEmptyBootstrap") };
  }
  return { ...base, execute: true, reason: t("migrationDialog.reason.sourceDetected") };
}

function planGroupKey(item: MigrationPlanItem): string {
  const unitId = item.unitId?.trim();
  if (!unitId) {
    return "__default__";
  }
  const segments = unitId
    .split("::")
    .map((segment) => segment.trim())
    .filter((segment) => segment.length > 0);
  if (segments.length < 2) {
    return "__default__";
  }
  return segments[1];
}

function planGroupLabel(key: string): string {
  if (key === "__default__") {
    return t("migrationDialog.pathGroup.default");
  }
  return t("migrationDialog.pathGroup.account", { account: key });
}

const candidateDisks = computed(() =>
  props.disks.filter((disk) => disk.is_mounted && disk.is_writable)
);

const migrationPlan = computed<MigrationPlanItem[]>(() => {
  if (!props.selectedApp) {
    return [];
  }
  return normalizeDetectedPaths(props.selectedApp).map((path, index) =>
    buildPlanItem(props.selectedApp as AppScanResult, path, index)
  );
});

const displayMigrationPlan = computed(() =>
  migrationPlan.value.filter((item) => !item.alreadyMigrated)
);

const migrationPlanGroups = computed(() => {
  const grouped = new Map<string, MigrationPlanItem[]>();
  for (const item of displayMigrationPlan.value) {
    const key = planGroupKey(item);
    const bucket = grouped.get(key) ?? [];
    bucket.push(item);
    grouped.set(key, bucket);
  }

  const groups = Array.from(grouped.entries()).map(([key, items]) => ({
    key,
    label: planGroupLabel(key),
    items
  }));
  groups.sort((left, right) => {
    if (left.key === "__default__") {
      return -1;
    }
    if (right.key === "__default__") {
      return 1;
    }
    return left.label.localeCompare(right.label);
  });
  return groups;
});

const showPlanGroupTabs = computed(
  () => migrationPlanGroups.value.filter((group) => group.key !== "__default__").length > 1
);

const visibleMigrationPlan = computed(() => {
  if (!showPlanGroupTabs.value) {
    return displayMigrationPlan.value;
  }
  if (!activePlanGroupKey.value) {
    return [];
  }
  return displayMigrationPlan.value.filter((item) => planGroupKey(item) === activePlanGroupKey.value);
});

const executablePlan = computed(() => migrationPlan.value.filter((item) => item.execute));
const visibleExecutablePlan = computed(() => visibleMigrationPlan.value.filter((item) => item.execute));
const skippedPlan = computed(
  () => migrationPlan.value.filter((item) => !item.execute && !item.alreadyMigrated)
);
const selectedExecutablePlan = computed(() => executablePlan.value);
const needsRiskConfirmation = computed(() =>
  selectedExecutablePlan.value.some((item) => item.requiresConfirmation)
);
const unitHintSelectedCount = computed(() =>
  showPlanGroupTabs.value ? visibleExecutablePlan.value.length : selectedExecutablePlan.value.length
);
const unitHintTotalCount = computed(() =>
  showPlanGroupTabs.value ? visibleExecutablePlan.value.length : executablePlan.value.length
);

const targetRootOptions = computed(() => {
  const options = targetDiskMount.value
    ? [
        {
          value: targetDiskMount.value,
          label: t("migrationDialog.targetRootDiskRoot", { root: targetDiskMount.value })
        },
        {
          value: `${targetDiskMount.value}/DataDock`,
          label: t("migrationDialog.targetRootRecommended", { root: targetDiskMount.value })
        },
        {
          value: `${targetDiskMount.value}/RelocatorData`,
          label: t("migrationDialog.targetRootRelocator", { root: targetDiskMount.value })
        }
      ]
    : [];

  if (targetRoot.value && !options.some((item) => item.value === targetRoot.value)) {
    options.unshift({
      value: targetRoot.value,
      label: t("migrationDialog.targetRootSystemPick", { root: targetRoot.value })
    });
  }
  return options;
});

const selectedDiskName = computed(() => {
  const disk = props.disks.find((item) => item.mount_point === targetDiskMount.value);
  return disk?.display_name ?? targetDiskMount.value;
});

const selectedAppSizeText = computed(() =>
  formatBytes(
    selectedExecutablePlan.value.reduce((sum, item) => sum + Math.max(0, item.sourceSizeBytes), 0)
  )
);

const riskWarningMessage = computed(() => {
  const warning = props.selectedApp?.migration_warning_i18n;
  const fallback = needsRiskConfirmation.value ? t("migrationDialog.riskWarning.defaultMessage") : "";
  if (!warning) {
    return fallback;
  }
  const activeLocale = locale.value.toLowerCase();
  const localeBase = activeLocale.split("-")[0];
  const direct = warning[activeLocale];
  if (typeof direct === "string" && direct.trim()) {
    return direct.trim();
  }
  const base = warning[localeBase];
  if (typeof base === "string" && base.trim()) {
    return base.trim();
  }
  const profileFallback = warning.zh ?? warning.en;
  if (typeof profileFallback === "string" && profileFallback.trim()) {
    return profileFallback.trim();
  }
  return fallback;
});

const riskWarningCountdownSeconds = computed(() => {
  const raw = props.selectedApp?.migration_warning_countdown_seconds;
  if (typeof raw !== "number" || !Number.isFinite(raw)) {
    return 3;
  }
  const normalized = Math.floor(raw);
  if (normalized <= 0) {
    return 0;
  }
  return normalized;
});

const hasRiskWarning = computed(() => needsRiskConfirmation.value);

const canStart = computed(() => {
  if (!props.selectedApp) {
    return false;
  }
  if (!targetRoot.value.trim()) {
    return false;
  }
  if (loading.value) {
    return false;
  }
  if (selectedExecutablePlan.value.length === 0) {
    return false;
  }
  if (needsRiskConfirmation.value && !confirmHighRisk.value) {
    return false;
  }
  return true;
});

function resetDialogState(): void {
  migrationStep.value = 0;
  progress.value = 0;
  confirmHighRisk.value = false;
  cleanupBackupAfterMigrate.value = true;
  loading.value = false;
  error.value = null;
  successResults.value = [];
  copiedPathKey.value = null;
  activePlanGroupKey.value = "";
  showRiskWarningModal.value = false;
  warningCountdownRemaining.value = 0;
  stopCopyFeedbackTimer();
  stopWarningCountdownTimer();

  const defaultDisk = candidateDisks.value[0]?.mount_point ?? "";
  targetDiskMount.value = defaultDisk;
  targetRoot.value = defaultDisk;
}

function stopWarningCountdownTimer(): void {
  if (warningCountdownTimer !== null) {
    window.clearInterval(warningCountdownTimer);
    warningCountdownTimer = null;
  }
}

function armRiskWarningIfNeeded(): void {
  stopWarningCountdownTimer();
  if (!hasRiskWarning.value) {
    showRiskWarningModal.value = false;
    warningCountdownRemaining.value = 0;
    return;
  }

  showRiskWarningModal.value = true;
  warningCountdownRemaining.value = Math.max(0, riskWarningCountdownSeconds.value);
  if (warningCountdownRemaining.value <= 0) {
    return;
  }

  warningCountdownTimer = window.setInterval(() => {
    if (warningCountdownRemaining.value <= 1) {
      warningCountdownRemaining.value = 0;
      stopWarningCountdownTimer();
      return;
    }
    warningCountdownRemaining.value -= 1;
  }, 1000);
}

function onConfirmRiskWarning(): void {
  if (warningCountdownRemaining.value > 0) {
    return;
  }
  showRiskWarningModal.value = false;
}

watch(
  () => [props.showModal, props.selectedAppId, candidateDisks.value.length],
  ([show]) => {
    if (show) {
      resetDialogState();
      armRiskWarningIfNeeded();
      return;
    }
    stopWarningCountdownTimer();
    showRiskWarningModal.value = false;
    warningCountdownRemaining.value = 0;
  },
  { immediate: true }
);

watch(
  () => [props.showModal, showPlanGroupTabs.value, migrationPlanGroups.value.map((group) => group.key).join("|")],
  ([show]) => {
    if (!show) {
      return;
    }
    if (!showPlanGroupTabs.value) {
      activePlanGroupKey.value = "";
      return;
    }
    const keys = migrationPlanGroups.value
      .filter((group) => group.key !== "__default__")
      .map((group) => group.key);
    if (!keys.includes(activePlanGroupKey.value)) {
      activePlanGroupKey.value = keys[0] ?? "";
    }
  },
  { immediate: true }
);

function stopProgressAnimation(): void {
  if (progressTimer !== null) {
    window.clearInterval(progressTimer);
    progressTimer = null;
  }
}

function stopCopyFeedbackTimer(): void {
  if (copyFeedbackTimer !== null) {
    window.clearTimeout(copyFeedbackTimer);
    copyFeedbackTimer = null;
  }
}

function startProgressAnimation(): void {
  stopProgressAnimation();
  progressTimer = window.setInterval(() => {
    if (progress.value >= 92) {
      return;
    }
    const next = progress.value + Math.floor(Math.random() * 8) + 3;
    progress.value = Math.min(92, next);
  }, 380);
}

function onSelectDisk(mountPoint: string): void {
  targetDiskMount.value = mountPoint;
  targetRoot.value = mountPoint;
}

async function onCopyPath(item: MigrationPlanItem): Promise<void> {
  const sourcePath = item.sourcePath?.trim();
  if (!sourcePath) {
    return;
  }

  try {
    if (!navigator.clipboard?.writeText) {
      throw new Error("clipboard API unavailable");
    }
    await navigator.clipboard.writeText(sourcePath);
    copiedPathKey.value = item.key;
    stopCopyFeedbackTimer();
    copyFeedbackTimer = window.setTimeout(() => {
      copiedPathKey.value = null;
      copyFeedbackTimer = null;
    }, 1200);
  } catch (err) {
    error.value = t("migrationDialog.errors.copyPathFailed", { error: formatCommandError(err) });
  }
}

async function onOpenInFinder(item: MigrationPlanItem): Promise<void> {
  const sourcePath = item.sourcePath?.trim();
  if (!sourcePath) {
    return;
  }

  try {
    await invoke("open_in_finder", { path: sourcePath });
  } catch (err) {
    error.value = t("migrationDialog.errors.openPathFailed", { error: formatCommandError(err) });
  }
}

async function onPickTargetRoot(): Promise<void> {
  if (!targetDiskMount.value) {
    error.value = t("migrationDialog.selectDiskFirst");
    return;
  }

  selectingTargetRoot.value = true;
  try {
    const picked = await open({
      directory: true,
      multiple: false,
      defaultPath: targetRoot.value || targetDiskMount.value || "/Volumes",
      title: t("migrationDialog.pickTitle")
    });
    if (!picked || Array.isArray(picked)) {
      return;
    }

    const onSelectedDisk =
      picked === targetDiskMount.value || picked.startsWith(`${targetDiskMount.value}/`);
    if (!onSelectedDisk) {
      error.value = t("migrationDialog.pathNotInDisk", { disk: targetDiskMount.value });
      return;
    }

    targetRoot.value = picked;
    error.value = null;
  } catch (err) {
    error.value = formatCommandError(err);
  } finally {
    selectingTargetRoot.value = false;
  }
}

async function onStartMigration(): Promise<void> {
  if (!canStart.value) {
    return;
  }

  loading.value = true;
  error.value = null;
  migrationStep.value = 1;
  progress.value = 5;
  startProgressAnimation();
  const batchTraceId = createBatchTraceId();

  const results: RelocationResult[] = [];
  try {
    for (let index = 0; index < selectedExecutablePlan.value.length; index += 1) {
      const item = selectedExecutablePlan.value[index];
      const request: MigrateRequest = {
        app_id: item.app.app_id,
        target_root: targetRoot.value.trim(),
        mode: item.mode,
        trace_id: batchTraceId,
        confirm_high_risk: confirmHighRisk.value,
        cleanup_backup_after_migrate: cleanupBackupAfterMigrate.value
      };
      if (item.unitId) {
        request.unit_id = item.unitId;
      }
      const result = await invoke<RelocationResult>("migrate_app", { req: request });
      results.push(result);

      const ratio = (index + 1) / selectedExecutablePlan.value.length;
      progress.value = Math.max(progress.value, Math.floor(ratio * 96));
    }

    stopProgressAnimation();
    successResults.value = results;
    progress.value = 100;
    setTimeout(() => {
      migrationStep.value = 2;
    }, 200);
  } catch (err) {
    stopProgressAnimation();
    migrationStep.value = 0;
    error.value = formatMigrationStartError(err);
  } finally {
    loading.value = false;
  }
}

function onFinish(): void {
  const uniqueAppIds = Array.from(
    new Set(successResults.value.map((item) => item.app_id).filter((value) => value.trim().length > 0))
  );
  const preferredLabel = props.selectedApp?.display_name?.trim() || "";
  const fallbackIds = uniqueAppIds.join(" / ");
  const label = preferredLabel || fallbackIds || t("migrationDialog.fallbackLabel");
  emit("done", t("migrationDialog.doneMessage", { label }));
}

onBeforeUnmount(() => {
  stopProgressAnimation();
  stopCopyFeedbackTimer();
  stopWarningCountdownTimer();
});
</script>

<template>
  <div
    v-if="props.showModal && props.selectedApp"
    class="fixed inset-0 bg-black/40 backdrop-blur-sm flex items-center justify-center z-50 p-4 overflow-y-auto"
  >
    <div class="bg-white rounded-2xl shadow-2xl w-full max-w-lg max-h-[92vh] overflow-hidden flex flex-col my-auto">
      <template v-if="showRiskWarningModal">
        <div class="p-6 border-b border-red-200 bg-red-50 flex items-start gap-4">
          <div class="w-12 h-12 bg-red-100 text-red-600 rounded-full flex items-center justify-center flex-shrink-0">
            <AlertCircle :size="24" />
          </div>
          <div>
            <h2 class="text-xl font-bold text-red-700">
              {{ t("migrationDialog.riskWarning.title") }}
            </h2>
            <p class="text-sm text-red-700 mt-1 whitespace-pre-line">{{ riskWarningMessage }}</p>
          </div>
        </div>
        <div class="p-4 border-t border-red-100 flex justify-end gap-3 bg-white">
          <button
            type="button"
            @click="emit('close')"
            class="px-4 py-2 text-gray-600 hover:bg-gray-100 rounded-lg font-medium transition-colors"
          >
            {{ t("common.cancelled") }}
          </button>
          <button
            type="button"
            data-test="risk-warning-confirm-btn"
            :disabled="warningCountdownRemaining > 0"
            @click="onConfirmRiskWarning"
            class="px-6 py-2 bg-red-600 hover:bg-red-700 disabled:bg-red-300 text-white rounded-lg font-medium transition-colors"
          >
            {{
              warningCountdownRemaining > 0
                ? t("migrationDialog.riskWarning.confirmWithCountdown", {
                    seconds: warningCountdownRemaining
                  })
                : t("migrationDialog.riskWarning.confirm")
            }}
          </button>
        </div>
      </template>
      <template v-else-if="migrationStep === 0">
        <div class="p-6 border-b border-gray-100 flex items-start gap-4">
          <div class="w-12 h-12 bg-blue-50 text-blue-500 rounded-full flex items-center justify-center flex-shrink-0">
            <ArrowRightLeft :size="24" />
          </div>
          <div>
            <h2 class="text-xl font-bold text-gray-900">
              {{ t("migrationDialog.title", { name: props.selectedApp.display_name }) }}
            </h2>
            <p class="text-sm text-gray-500 mt-1">{{ t("migrationDialog.subtitle") }}</p>
          </div>
        </div>

        <div class="p-6 bg-gray-50/50 space-y-4 flex-1 min-h-0 overflow-y-auto">
          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">{{
              t("migrationDialog.sizeLabel")
            }}</label>
            <div class="bg-white p-3 rounded-lg border border-gray-200 text-lg font-mono font-bold text-gray-800">
              {{ selectedAppSizeText }}
            </div>
          </div>

          <div>
            <div class="flex items-center justify-between gap-3 mb-2">
              <label class="block text-sm font-medium text-gray-700">{{
                t("migrationDialog.unitLabel")
              }}</label>
              <div class="text-xs text-gray-500">
                {{
                  t("migrationDialog.unitHint", {
                    selected: unitHintSelectedCount,
                    total: unitHintTotalCount
                  })
                }}
              </div>
            </div>
            <div v-if="showPlanGroupTabs" class="mb-2 flex items-center gap-2 overflow-x-auto">
              <button
                v-for="group in migrationPlanGroups.filter((item) => item.key !== '__default__')"
                :key="group.key"
                :class="`px-3 py-1.5 rounded-lg text-xs whitespace-nowrap border ${
                  activePlanGroupKey === group.key
                    ? 'bg-blue-500 text-white border-blue-500'
                    : 'bg-white text-gray-700 border-gray-200 hover:border-blue-300'
                }`"
                @click="activePlanGroupKey = group.key"
              >
                {{ group.label }}
              </button>
            </div>
            <div class="space-y-2">
              <div
                v-for="item in visibleMigrationPlan"
                :key="item.key"
                :class="`flex items-start justify-between gap-3 p-3 rounded-lg border ${item.execute ? 'bg-white border-gray-200' : 'bg-gray-100 border-gray-200'}`"
              >
                <div class="min-w-0">
                  <div class="text-sm font-medium text-gray-800">{{ item.unitLabel }}</div>
                  <div class="flex items-center gap-2 min-w-0">
                    <div class="text-xs text-gray-500 font-mono truncate" :title="item.sourcePath || t('app.pathFallback')">
                      {{ item.sourcePath || t("app.pathFallback") }}
                    </div>
                    <button
                      type="button"
                      data-test="open-path-btn"
                      :disabled="loading || !(item.sourcePath || '').trim()"
                      @pointerdown.stop.prevent
                      @mousedown.stop.prevent
                      @click.stop.prevent="onOpenInFinder(item)"
                      class="text-xs text-gray-600 hover:text-gray-800 whitespace-nowrap disabled:text-gray-400"
                    >
                      {{ t("migrationDialog.openInFinder") }}
                    </button>
                    <button
                      type="button"
                      data-test="copy-path-btn"
                      :disabled="loading || !(item.sourcePath || '').trim()"
                      @pointerdown.stop.prevent
                      @mousedown.stop.prevent
                      @click.stop.prevent="onCopyPath(item)"
                      class="text-xs text-blue-600 hover:text-blue-700 whitespace-nowrap disabled:text-gray-400"
                    >
                      {{
                        copiedPathKey === item.key
                          ? t("migrationDialog.pathCopied")
                          : t("migrationDialog.copyPath")
                      }}
                    </button>
                  </div>
                  <div class="text-xs mt-1" :class="item.execute ? 'text-green-700' : 'text-gray-500'">
                    {{ item.reason }}
                  </div>
                </div>
                <div class="text-xs text-gray-500 whitespace-nowrap">{{ formatBytes(item.sourceSizeBytes) }}</div>
              </div>
            </div>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">{{
              t("migrationDialog.diskLabel")
            }}</label>
            <p class="text-xs text-gray-500 mb-2">{{ t("migrationDialog.diskHint") }}</p>
            <div class="space-y-2">
              <label
                v-for="disk in candidateDisks"
                :key="disk.mount_point"
                :class="`flex items-center justify-between p-3 rounded-lg border cursor-pointer transition-colors ${targetDiskMount === disk.mount_point ? 'border-blue-500 bg-blue-50/30' : 'border-gray-200 bg-white hover:border-blue-300'}`"
              >
                <div class="flex items-center gap-3">
                  <input
                    type="radio"
                    name="targetDisk"
                    :value="disk.mount_point"
                    :checked="targetDiskMount === disk.mount_point"
                    @change="onSelectDisk(disk.mount_point)"
                    class="w-4 h-4 text-blue-600 focus:ring-blue-500"
                  />
                  <HardDrive :class="targetDiskMount === disk.mount_point ? 'text-blue-500' : 'text-gray-400'" :size="20" />
                  <span class="font-medium text-gray-800">{{ disk.display_name }}</span>
                </div>
                <span class="text-sm text-gray-500">{{
                  t("migrationDialog.diskFree", { size: formatBytes(disk.free_bytes) })
                }}</span>
              </label>
            </div>
            <p v-if="candidateDisks.length === 0" class="mt-2 text-xs text-red-600">
              {{ t("migrationDialog.diskNone") }}
            </p>
          </div>

          <div>
            <label class="block text-sm font-medium text-gray-700 mb-2">{{
              t("migrationDialog.targetRootLabel")
            }}</label>
            <div class="flex items-center gap-2">
              <select
                class="flex-1 h-10 border border-gray-200 rounded-lg px-3 text-sm"
                :value="targetRoot"
                @change="targetRoot = ($event.target as HTMLSelectElement).value"
              >
                <option v-for="option in targetRootOptions" :key="option.value" :value="option.value">
                  {{ option.label }}
                </option>
              </select>
              <button
                type="button"
                :disabled="loading || selectingTargetRoot"
                @click="onPickTargetRoot"
                class="h-10 px-3 inline-flex items-center border border-gray-200 rounded-lg text-sm text-gray-700 hover:bg-gray-100 whitespace-nowrap"
              >
                {{ selectingTargetRoot ? t("migrationDialog.picking") : t("migrationDialog.pickButton") }}
              </button>
            </div>
          </div>

          <label v-if="needsRiskConfirmation" class="flex items-start gap-2 text-sm text-gray-700">
            <input
              v-model="confirmHighRisk"
              data-test="confirm-high-risk-checkbox"
              type="checkbox"
              class="mt-0.5"
            />
            <span>{{ t("migrationDialog.confirmHighRisk") }}</span>
          </label>

          <label class="flex items-start gap-2 text-sm text-gray-700">
            <input v-model="cleanupBackupAfterMigrate" data-test="cleanup-checkbox" type="checkbox" class="mt-0.5" />
            <span>{{ t("migrationDialog.cleanupBackup") }}</span>
          </label>

          <div class="bg-yellow-50 p-3 rounded-lg flex items-start gap-2 border border-yellow-100">
            <AlertCircle :size="16" class="text-yellow-600 mt-0.5" />
            <p class="text-xs text-yellow-700 leading-relaxed">
              {{ t("migrationDialog.warning", { name: props.selectedApp.display_name }) }}
            </p>
          </div>

          <div v-if="skippedPlan.length > 0" class="text-xs text-gray-500 leading-5">
            <div class="font-medium text-gray-700">{{ t("migrationDialog.skippedTitle") }}</div>
            <div v-for="item in skippedPlan" :key="item.key">{{ item.unitLabel }}：{{ item.reason }}</div>
          </div>

          <div v-if="error" class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
            {{ error }}
          </div>
        </div>

        <div class="p-4 border-t border-gray-100 flex justify-end gap-3 bg-white">
          <button
            type="button"
            :disabled="loading"
            @click="emit('close')"
            class="px-4 py-2 text-gray-600 hover:bg-gray-100 rounded-lg font-medium transition-colors"
          >
            {{ t("migrationDialog.cancel") }}
          </button>
          <button
            type="button"
            :disabled="!canStart"
            @click="onStartMigration"
            class="px-6 py-2 bg-blue-500 hover:bg-blue-600 disabled:bg-gray-300 text-white rounded-lg font-medium shadow-sm shadow-blue-500/30 transition-colors"
          >
            {{ loading ? t("migrationDialog.migratingBtn") : t("migrationDialog.start") }}
          </button>
        </div>
      </template>

      <div v-else-if="migrationStep === 1" class="p-10 flex flex-col items-center justify-center text-center">
        <div class="w-16 h-16 border-4 border-gray-100 border-t-blue-500 rounded-full animate-spin mb-6"></div>
        <h2 class="text-xl font-bold text-gray-900 mb-2">{{ t("migrationDialog.migratingTitle") }}</h2>
        <p class="text-sm text-gray-500 mb-6">
          {{ t("migrationDialog.migratingSubtitle", { count: selectedExecutablePlan.length, disk: selectedDiskName }) }}
        </p>

        <div class="w-full bg-gray-100 rounded-full h-3 mb-2 overflow-hidden">
          <div class="bg-blue-500 h-full transition-all duration-300 ease-out" :style="{ width: `${progress}%` }"></div>
        </div>
        <div class="w-full flex justify-between text-xs font-mono text-gray-400">
          <span>{{ progress }}%</span>
          <span>{{ t("migrationDialog.keepDiskOnline") }}</span>
        </div>
      </div>

      <div v-else-if="migrationStep === 2" class="p-10 flex flex-col items-center justify-center text-center">
        <div class="w-16 h-16 bg-green-100 text-green-500 rounded-full flex items-center justify-center mb-6">
          <CheckCircle2 :size="32" />
        </div>
        <h2 class="text-xl font-bold text-gray-900 mb-2">{{ t("migrationDialog.successTitle") }}</h2>
        <p class="text-sm text-gray-500 mb-8">
          {{
            t("migrationDialog.successText", {
              name: props.selectedApp.display_name,
              disk: selectedDiskName,
              count: successResults.length
            })
          }}
        </p>

        <button
          type="button"
          @click="onFinish"
          class="w-full py-3 bg-gray-900 hover:bg-black text-white rounded-xl font-medium transition-colors"
        >
          {{ t("migrationDialog.finish") }}
        </button>
      </div>
    </div>
  </div>
</template>
