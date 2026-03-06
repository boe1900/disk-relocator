<script setup lang="ts">
import { invoke } from "@tauri-apps/api/core";
import { ref } from "vue";
import type { ExportLogsRequest, ExportLogsResult } from "../types/contracts";

const relocationId = ref("");
const traceId = ref("");
const outputPath = ref("");
const payload = ref<ExportLogsResult | null>(null);
const error = ref<string | null>(null);

async function onExportLogs(): Promise<void> {
  error.value = null;
  const req: ExportLogsRequest = {};
  if (relocationId.value.trim()) {
    req.relocation_id = relocationId.value.trim();
  }
  if (traceId.value.trim()) {
    req.trace_id = traceId.value.trim();
  }
  if (outputPath.value.trim()) {
    req.output_path = outputPath.value.trim();
  }

  try {
    payload.value = await invoke<ExportLogsResult>("export_operation_logs", { req });
  } catch (err) {
    error.value = String(err);
  }
}
</script>

<template>
  <section class="panel">
    <h2>迁移日志导出（T09）</h2>
    <p class="muted">调用 <code>export_operation_logs</code>，导出结构化步骤日志（含 trace id / 耗时 / 错误码）。</p>
    <label class="field">
      relocation_id（可选）
      <input v-model="relocationId" type="text" placeholder="reloc_xxx" />
    </label>
    <label class="field">
      trace_id（可选）
      <input v-model="traceId" type="text" placeholder="tr_xxx" />
    </label>
    <label class="field">
      output_path（可选）
      <input v-model="outputPath" type="text" placeholder="/tmp/export.json" />
    </label>
    <button type="button" @click="onExportLogs">导出日志</button>
    <pre v-if="payload">{{ JSON.stringify(payload, null, 2) }}</pre>
    <pre v-else-if="error">{{ error }}</pre>
  </section>
</template>
