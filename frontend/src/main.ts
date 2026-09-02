import './index.css';
import { mount } from 'svelte';
import App from './App.svelte';

const target = document.getElementById('app');
if (!target) {
  throw new Error('Root #app element not found');
}

const app = mount(App, {
  target,
});

export default app;
