import { defaultConfig } from "../../../src/hooks/useConfig";
import { DEFAULT_THEME, isLightColor, themeToCSS } from "../../../src/lib/themeCss";

const root = document.documentElement;
const light = isLightColor(DEFAULT_THEME.background);
root.style.colorScheme = light ? "light" : "dark";
root.dataset.ctTheme = light ? "light" : "dark";
for (const [name, value] of Object.entries(themeToCSS(DEFAULT_THEME))) root.style.setProperty(name, value);
root.style.fontSize = `${defaultConfig().font.size + 2}px`;
