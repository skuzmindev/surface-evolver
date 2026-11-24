import { EditorView }                      from '@codemirror/view'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { tags }                              from '@lezer/highlight'
import type { Extension }                    from '@codemirror/state'

// A daisyUI theme colour (`base-content`, `primary`, …), with optional alpha
const oc = (name: string, a?: number) =>
  a != null
    ? `color-mix(in oklab, var(--color-${name}) ${a * 100}%, transparent)`
    : `var(--color-${name})`

export const seTheme: Extension = [
  EditorView.theme({
    '&': {
      color:           oc('base-content'),
      backgroundColor: oc('base-100'),
      height: '100%',
      fontSize: '13px',
    },
    '.cm-content': {
      caretColor: oc('primary'),
    },
    '&.cm-focused .cm-cursor': {
      borderLeftColor: oc('primary'),
    },
    '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': {
      backgroundColor: oc('base-300'),
    },
    '::selection': {
      backgroundColor: oc('base-300'),
    },
    '.cm-activeLine': {
      backgroundColor: oc('base-200', 0.7),
    },
    '.cm-gutters': {
      backgroundColor: oc('base-200'),
      color:           oc('base-content', 0.3),
      borderRight:     `1px solid ${oc('base-300')}`,
    },
    '.cm-activeLineGutter': {
      backgroundColor: oc('base-200'),
      color:           oc('base-content', 0.6),
    },
    '.cm-lineNumbers .cm-gutterElement': {
      padding: '0 10px 0 4px',
    },
    '.cm-scroller': {
      fontFamily: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace',
      overflow: 'auto',
    },
    '.cm-matchingBracket': {
      backgroundColor: oc('primary', 0.18),
      outline:         `1px solid ${oc('primary', 0.35)}`,
    },
    '.cm-tooltip': {
      backgroundColor: oc('base-200'),
      border:          `1px solid ${oc('base-300')}`,
      color:           oc('base-content'),
    },
    '.cm-foldPlaceholder': {
      backgroundColor: oc('base-300'),
      border:          'none',
      color:           oc('base-content', 0.6),
    },
  }),

  syntaxHighlighting(HighlightStyle.define([
    { tag: tags.keyword,      color: oc('primary'),        fontWeight: '600'  },
    { tag: tags.string,       color: oc('success')                           },
    { tag: tags.number,       color: oc('warning')                           },
    { tag: tags.comment,      color: oc('base-content', 0.38), fontStyle: 'italic' },
    { tag: tags.variableName, color: oc('base-content', 0.82)                     },
    { tag: tags.operator,     color: oc('accent')                            },
    { tag: tags.punctuation,  color: oc('base-content', 0.45)                     },
    { tag: tags.literal,      color: oc('warning')                           },
  ])),
]
