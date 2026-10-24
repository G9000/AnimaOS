import { StrictMode } from 'react';
import * as ReactDOM from 'react-dom/client';
import { PlaygroundEntry } from './PlaygroundEntry';
import './styles.css';

const root = ReactDOM.createRoot(
  document.getElementById('root') as HTMLElement
);

root.render(
  <StrictMode>
    <PlaygroundEntry />
  </StrictMode>
);
