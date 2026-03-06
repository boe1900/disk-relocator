<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import type { ReconcileRequest, ReconcileResult } from "../types/contracts";

const payload = ref<ReconcileResult | null>(null);
const error = ref<string | null>(null);
const loading = ref(false);

async function onRun(applySafeFixes: boolean): Promise<void> {
  loading.value = true;
  error.value = null;
  const req: ReconcileRequest = {
    apply_safe_fixes: applySafeFixes,
    limit: 500
  };

  try {
    payload.value = await invoke<ReconcileResult>("reconcile_relocations", { req });
  } catch (err) {
    error.value = String(err);
  } finally {
    loading.value = false;
  }
}
</script>

<template>
  <section class="panel">
    <h2>对账任务（T12）</h2>
    <p class="muted">
      元数据 vs 文件系统漂移扫描。支持“仅扫描”与“扫描并执行 safe-fix（仅无损修复）”。
    </p>
    <div style="display: flex; gap: 8px; margin-top: 8px; flex-wrap: wrap">
      <button type="button" :disabled="loading" @click="onRun(false)">
        {{ loading ? "执行中..." : "运行对账扫描" }}
      </button>
      <button type="button" :disabled="loading" @click="onRun(true)">
        {{ loading ? "执行中..." : "运行并执行 safe-fix" }}
      </button>
    </div>
    <pre v-if="payload">{{ JSON.stringify(payload, null, 2) }}</pre>
    <pre v-else-if="error">{{ error }}</pre>
  </section>
</template>
