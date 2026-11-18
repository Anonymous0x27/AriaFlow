import { defineStore } from "pinia";
import { api, type DownloadTask, type StatusResponse } from "../lib/api";

export type TaskFilter = "all" | "active" | "queued" | "completed" | "failed" | "paused";

export const FILTER_STATUSES: Record<TaskFilter, string> = {
  all: "",
  active: "downloading,connecting,merging,verifying",
  queued: "pending,queued,retrying,waiting",
  completed: "completed",
  failed: "failed,cancelled",
  paused: "paused",
};

interface DownloadsState {
  tasks: DownloadTask[];
  filter: TaskFilter;
  /** Overview-only: also fetch paused tasks when filter is "active". */
  includePausedInActive: boolean;
  /** Overview-only: also fetch newly created (pending/queued/waiting/retrying) tasks when filter is "active". */
  includeNewInActive: boolean;
  /** Global default for the "delete downloaded files" checkbox in confirm dialogs. */
  deleteFilesOnRemove: boolean;
  search: string;
  connected: boolean;
  lastError: string | null;
  counts: StatusResponse["counts"] | null;
  daemonVersion: string;
  loading: boolean;
  pollTimer: number | null;
}

export const useDownloadsStore = defineStore("downloads", {
  state: (): DownloadsState => ({
    tasks: [],
    filter: "all",
    includePausedInActive: false,
    includeNewInActive: false,
    deleteFilesOnRemove: false,
    search: "",
    connected: false,
    lastError: null,
    counts: null,
    daemonVersion: "",
    loading: false,
    pollTimer: null,
  }),

  getters: {
    visibleTasks(state): DownloadTask[] {
      const needle = state.search.trim().toLowerCase();
      if (!needle) return state.tasks;
      return state.tasks.filter(
        (t) =>
          t.filename.toLowerCase().includes(needle) ||
          t.url.toLowerCase().includes(needle) ||
          t.category.toLowerCase().includes(needle),
      );
    },
  },

  actions: {
    /** Load global UI prefs (delete-files default) once at startup. */
    async loadGlobalPrefs() {
      try {
        const s = await api.settings();
        this.deleteFilesOnRemove = s.deleteFilesOnRemove ?? false;
      } catch {
        // daemon offline — keep the client-side default
      }
    },

    async refresh() {
      this.loading = true;
      try {
        const params: Record<string, string> = {};
        let statuses = FILTER_STATUSES[this.filter];
        if (this.filter === "active") {
          const extra: string[] = [];
          if (this.includeNewInActive) extra.push("pending,queued,waiting,retrying");
          if (this.includePausedInActive) extra.push("paused");
          if (extra.length) statuses = `${statuses},${extra.join(",")}`;
        }
        if (statuses) params.status = statuses;
        if (this.search.trim()) params.q = this.search.trim();
        const [tasks, status] = await Promise.all([api.list(params), api.status()]);
        this.tasks = tasks;
        this.counts = status.counts;
        this.daemonVersion = status.version;
        this.connected = true;
        this.lastError = null;
      } catch (err) {
        this.connected = false;
        this.lastError = err instanceof Error ? err.message : String(err);
      } finally {
        this.loading = false;
      }
    },

    startPolling(intervalMs = 1500) {
      this.stopPolling();
      void this.refresh();
      this.pollTimer = window.setInterval(() => void this.refresh(), intervalMs);
    },

    stopPolling() {
      if (this.pollTimer !== null) {
        window.clearInterval(this.pollTimer);
        this.pollTimer = null;
      }
    },

    setFilter(filter: TaskFilter) {
      this.filter = filter;
      void this.refresh();
    },

    async add(url: string): Promise<void> {
      const trimmed = url.trim();
      if (!trimmed) return;
      await api.add(trimmed);
      await this.refresh();
    },

    async togglePauseResume(task: DownloadTask): Promise<void> {
      if (task.status === "paused") {
        await api.resume(task.id);
      } else {
        await api.pause(task.id);
      }
      await this.refresh();
    },

    async remove(task: DownloadTask, deleteFile = false): Promise<void> {
      await api.remove(task.id, deleteFile);
      await this.refresh();
    },

    /** Run one per-id action across a selection; returns success/failure counts. */
    async bulk(
      ids: string[],
      action: "pause" | "resume" | "retry" | "remove",
      options: { deleteFile?: boolean } = {},
    ): Promise<{ done: number; failed: number }> {
      let failed = 0;
      for (const id of ids) {
        try {
          if (action === "pause") await api.pause(id);
          else if (action === "resume") await api.resume(id);
          else if (action === "retry") await api.retry(id);
          else await api.remove(id, options.deleteFile ?? false);
        } catch {
          failed += 1;
        }
      }
      await this.refresh();
      return { done: ids.length - failed, failed };
    },

    async retry(task: DownloadTask): Promise<void> {
      await api.retry(task.id);
      await this.refresh();
    },
  },
});
