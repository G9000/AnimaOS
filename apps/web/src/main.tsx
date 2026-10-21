import { StrictMode } from 'react';
import * as ReactDOM from 'react-dom/client';
import '@fontsource-variable/geist';
import '@fontsource-variable/geist-mono';
import '@fontsource-variable/fraunces';
import { App } from './app/app';
import { claimConsoleSession } from './lib/console-session';
import './styles.css';

const consoleClaim = claimConsoleSession();

const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);

root.render(
  <StrictMode>
    <App consoleClaim={consoleClaim} />
  </StrictMode>
);
