import tailwindcss from 'tailwindcss';
import autoprefixer from 'autoprefixer';

// Text sizes written in px (text-[13px], plain CSS) become rem, so Settings › Interface size
// scales them with the root font size together with the rem-based text-xs/sm/… sizes.
export const remFontSize = {
  postcssPlugin: 'rem-font-size',
  Declaration: {
    'font-size': (decl) => {
      decl.value = decl.value.replace(/(\d*\.?\d+)px\b/g, (_, n) => `${Number(n) / 16}rem`);
    },
  },
};

export default {
  plugins: [tailwindcss, autoprefixer, remFontSize],
};
