import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';
import { useServerStore } from './stores/server';

const app = createApp(App);
app.use(createPinia());
app.mount('#app');

const store = useServerStore();
store.loadAll();
store.startLiveUpdates();
