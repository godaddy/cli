import type { BundleRule } from "../../types.ts";

/**
 * SEC112: DOM escape operations in checkout/embed UI extension bundles.
 *
 * Phase 1 DOM bundle extensions are trusted in-page code and must render only
 * inside the host-provided container. Start strict: block obvious page-level DOM,
 * storage, and navigation access so unsafe bundles cannot be deployed by older
 * templates or custom source.
 */
export const SEC112_DOM_ESCAPE: BundleRule = {
  id: "SEC112",
  severity: "block",
  title: "DOM escape operation in UI extension bundle",
  description:
    "UI extension bundle accesses page-level DOM, storage, or navigation APIs outside the host-provided container",
  sourceRuleId: "SEC012",
  patterns: [
    /\bdocument\.body\b/g,
    /\bdocument\.documentElement\b/g,
    /\bdocument\.head\b/g,
    /\bdocument\.forms\b/g,
    /\bdocument\.images\b/g,
    /\bdocument\.links\b/g,
    /\bdocument\.scripts\b/g,
    /\bdocument\.cookie\b/g,
    /\bdocument\.activeElement\b/g,
    /\bdocument\.children\b/g,
    /\bdocument\.firstElementChild\b/g,
    /\bdocument\.write\s*\(/g,
    /\bdocument\.querySelector\s*\(/g,
    /\bdocument\.querySelectorAll\s*\(/g,
    /\bdocument\.getElementById\s*\(/g,
    /\bdocument\.getElementsByClassName\s*\(/g,
    /\bdocument\.getElementsByName\s*\(/g,
    /\bdocument\.getElementsByTagName\s*\(/g,
    /\bdocument\.getElementsByTagNameNS\s*\(/g,
    /\bwindow\.document\b/g,
    /\bwindow\.location\b/g,
    /\bwindow\.open\s*\(/g,
    /\bwindow\.location\.assign\s*\(/g,
    /\bwindow\.location\.replace\s*\(/g,
    /\bglobalThis\.document\b/g,
    /\bglobalThis\.location\b/g,
    /\blocation\.href\b/g,
    /\blocation\.assign\s*\(/g,
    /\blocation\.replace\s*\(/g,
    /\bhistory\.pushState\s*\(/g,
    /\bhistory\.replaceState\s*\(/g,
    /\btop\.document\b/g,
    /\btop\.location\b/g,
    /\bparent\.document\b/g,
    /\bparent\.location\b/g,
    /\bElement\.prototype\b/g,
    /\bNode\.prototype\b/g,
    /\blocalStorage\b/g,
    /\bsessionStorage\b/g,
    /\bcontainer\.ownerDocument\b/g,
    /\bcontainer\.parentElement\b/g,
    /\bcontainer\.parentNode\b/g,
    /\bcontainer\.closest\s*\(/g,
  ],
};
