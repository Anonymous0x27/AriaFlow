import { createRouter, createWebHashHistory } from "vue-router";
import OverviewView from "../views/OverviewView.vue";
import DownloadsView from "../views/DownloadsView.vue";
import HistoryView from "../views/HistoryView.vue";
import QueueView from "../views/QueueView.vue";
import BrowserView from "../views/BrowserView.vue";
import SettingsView from "../views/SettingsView.vue";
import AboutView from "../views/AboutView.vue";

// Hash history keeps the app working from file:// (Tauri) without a server.
// `meta.titleKey` resolves through the i18n catalog (lib/messages.ts).
export const router = createRouter({
  history: createWebHashHistory(),
  routes: [
    { path: "/", name: "overview", component: OverviewView, meta: { titleKey: "route.overview" } },
    { path: "/downloads", name: "downloads", component: DownloadsView, meta: { titleKey: "route.downloads" } },
    { path: "/downloads/:filter", name: "downloads-filtered", component: DownloadsView, meta: { titleKey: "route.downloads" } },
    { path: "/history", name: "history", component: HistoryView, meta: { titleKey: "route.history" } },
    { path: "/queue", name: "queue", component: QueueView, meta: { titleKey: "route.queue" } },
    { path: "/browser", name: "browser", component: BrowserView, meta: { titleKey: "route.browser" } },
    { path: "/settings", name: "settings", component: SettingsView, meta: { titleKey: "route.settings" } },
    { path: "/about", name: "about", component: AboutView, meta: { titleKey: "route.about" } },
    { path: "/:pathMatch(.*)*", redirect: "/" },
  ],
});
