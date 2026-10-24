import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { VillageApp } from './App';
// oxlint-disable-next-line import/no-unassigned-import -- Vite loads the game stylesheet for its side effect.
import './style.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <VillageApp />
  </StrictMode>,
);
