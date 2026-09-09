import { mount } from 'svelte';
import { syncFavicon } from './lib/api';
import App from './App.svelte';
import './app.css';

const saved = localStorage.getItem('coduos-theme');
if (saved === 'light') document.documentElement.dataset.theme = 'light';
else document.documentElement.dataset.theme = 'dark';
syncFavicon();

mount(App, { target: document.getElementById('app')! });
