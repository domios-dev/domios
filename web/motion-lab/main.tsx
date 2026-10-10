import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { MotionLab } from './MotionLab';
import './tokens.css';
import './layout.css';
import './motion.css';

createRoot(document.getElementById('root')!).render(<StrictMode><MotionLab /></StrictMode>);
