<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { onBeforeUnmount, onMounted, ref } from "vue";
import type {
  DiskStatus,
  HealthEvent,
  HealthEventsRequest,
  HealthStatus,
  RollbackRequest
} from "../types/contracts";

const diskPayload = ref<DiskStatus[]>([]);
const healthPayload = ref<HealthStatus[]>([]);
const historyPayload = ref<HealthEvent[]>([]);
const actioningRelocationId = ref<string | null>(null);
const error = ref<string | null>(null);
const info = ref<string | null>(null);
let timer: number | undefined;

function guidanceFor(status: HealthStatus): { title: string; steps: string[]; canRollback: boolean } {
  const code = status.checks[0]?.code ?? "HEALTH_UNKNOWN";
  switch (code) {
    case "HEALTH_SYMLINK_MISSING":
      return {
        title: "源路径软链接缺失（broken）",
        steps: ["先关闭目标应用。", "执行一键回滚恢复源路径。", "回滚后重新执行健康检查确认恢复。"],
        canRollback: true
      };
    case "HEALTH_DISK_OFFLINE":
      return {
        title: "外接盘离线（degraded）",
        steps: ["重新挂载目标外接盘。", "确认挂载点可见并可写。", "执行健康检查刷新状态。"],
        canRollback: false
      };
    case "HEALTH_TARGET_MISSING":
      return {
        title: "目标目录缺失（broken）",
        steps: ["确认外接盘是否仍包含目标目录。", "若目标目录不可恢复，执行一键回滚。", "完成后重新健康检查。"],
        canRollback: true
      };
    case "HEALTH_TARGET_READONLY":
      return {
        title: "目标目录只读（degraded）",
        steps: ["解除卷只读状态或修复权限。", "确认目录可写。", "执行健康检查刷新状态。"],
        canRollback: false
      };
    case "HEALTH_RW_PROBE_FAILED":
      return {
        title: "读写探针失败（degraded）",
        steps: ["检查目录权限与空间。", "排除占用后重试。", "执行健康检查确认恢复。"],
        canRollback: false
      };
    case "HEALTH_METADATA_DRIFT":
      return {
        title: "元数据漂移（degraded）",
        steps: ["系统检测到元数据与软链接不一致。", "优先执行一键回滚回到稳定态。", "回滚后再评估是否重新迁移。"],
        canRollback: true
      };
    default:
      return {
        title: `异常状态（${code}）`,
        steps: ["先执行健康检查刷新。", "若持续异常，执行一键回滚。", "仍失败时查看操作日志定位问题。"],
        canRollback: true
      };
  }
}

async function refreshPanel(): Promise<void> {
  error.value = null;
  try {
    const req: HealthEventsRequest = { limit: 30 };
    const [disk, health, history] = await Promise.all([
      invoke<DiskStatus[]>("get_disk_status"),
      invoke<HealthStatus[]>("check_health"),
      invoke<HealthEvent[]>("list_health_events", { req })
    ]);
    diskPayload.value = disk;
    healthPayload.value = health;
    historyPayload.value = history;
  } catch (err) {
    error.value = String(err);
  }
}

async function onCheckDisk(): Promise<void> {
  await refreshPanel();
}

async function onCheckHealth(): Promise<void> {
  await refreshPanel();
}

async function onRollback(relocationId: string): Promise<void> {
  info.value = null;
  error.value = null;
  actioningRelocationId.value = relocationId;
  const req: RollbackRequest = { relocation_id: relocationId, force: true };
  try {
    await invoke("rollback_relocation", { req });
    info.value = `已执行回滚：${relocationId}`;
    await refreshPanel();
  } catch (err) {
    error.value = String(err);
  } finally {
    actioningRelocationId.value = null;
  }
}

onMounted(async () => {
  await refreshPanel();
  timer = window.setInterval(() => {
    void refreshPanel();
  }, 10000);
});

onBeforeUnmount(() => {
  if (timer !== undefined) {
    window.clearInterval(timer);
  }
});
</script>

<template>
  <section class="panel">
    <h2>健康告警面板</h2>
    <p class="muted">
      10 秒自动刷新，支持实时检查、异常恢复指引与一键回滚。调用
      <code>get_disk_status</code>/<code>check_health</code>/<code>list_health_events</code>。
    </p>
    <div style="display: flex; gap: 8px; margin-top: 8px">
      <button type="button" @click="onCheckDisk">检查磁盘状态</button>
      <button type="button" @click="onCheckHealth">检查健康状态</button>
    </div>
    <pre v-if="diskPayload.length > 0">{{ JSON.stringify(diskPayload, null, 2) }}</pre>
    <div v-for="status in healthPayload" :key="status.relocation_id + status.observed_at" class="health-card">
      <div class="health-card-head">
        <strong>{{ status.app_id }}</strong>
        <span>{{ status.state }}</span>
      </div>
      <p class="muted" style="margin: 6px 0 0">
        {{ guidanceFor(status).title }} · {{ status.checks[0]?.code ?? "HEALTH_UNKNOWN" }}
      </p>
      <ul class="steps">
        <li v-for="step in guidanceFor(status).steps" :key="step">{{ step }}</li>
      </ul>
      <div style="display: flex; gap: 8px; flex-wrap: wrap">
        <button type="button" @click="onCheckHealth">重新检测</button>
        <button
          v-if="guidanceFor(status).canRollback"
          type="button"
          :disabled="actioningRelocationId === status.relocation_id"
          @click="onRollback(status.relocation_id)"
        >
          {{ actioningRelocationId === status.relocation_id ? "回滚中..." : "执行一键回滚" }}
        </button>
      </div>
    </div>
    <h3 style="margin-top: 12px">健康事件历史</h3>
    <pre v-if="historyPayload.length > 0">{{ JSON.stringify(historyPayload, null, 2) }}</pre>
    <pre v-if="info">{{ info }}</pre>
    <pre v-if="error">{{ error }}</pre>
  </section>
</template>
