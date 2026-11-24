import { StreamLanguage, StringStream } from '@codemirror/language'

const KEYWORDS = new Set([
  'vertices', 'edges', 'faces', 'facets', 'bodies', 'constraints',
  'boundary', 'quantity', 'method_instance', 'parameter', 'define',
  'procedure', 'function', 'foreach', 'do', 'while', 'if', 'then',
  'else', 'return', 'print', 'printf', 'list', 'refine', 'delete',
  'set', 'unset', 'fix', 'unfix', 'show', 'not', 'and', 'or',
  'surface_tension', 'energy', 'content', 'volume', 'pressure',
  'density', 'color', 'fixed', 'constraint', 'on_constraint',
  'where', 'with', 'sqrt', 'sin', 'cos', 'tan', 'exp', 'log', 'abs',
])

interface State { inBlockComment: boolean }

export const feLanguage = StreamLanguage.define<State>({
  name: 'surface-evolver',

  startState: (): State => ({ inBlockComment: false }),

  token(stream: StringStream, state: State): string | null {
    if (state.inBlockComment) {
      if (stream.match('*/')) { state.inBlockComment = false; return 'comment' }
      stream.next()
      return 'comment'
    }

    if (stream.eatSpace()) return null

    if (stream.match('//')) { stream.skipToEnd(); return 'comment' }

    if (stream.match('/*')) { state.inBlockComment = true; return 'comment' }

    if (stream.peek() === '"') {
      stream.next()
      while (!stream.eol()) {
        const ch = stream.next()
        if (ch === '"') break
        if (ch === '\\') stream.next()
      }
      return 'string'
    }

    if (stream.match(/^-?\d+(\.\d*)?([eE][+-]?\d+)?/)) return 'number'
    if (stream.match(/^-?\.\d+([eE][+-]?\d+)?/))       return 'number'

    if (stream.match(/^[a-zA-Z_][a-zA-Z0-9_]*/)) {
      return KEYWORDS.has(stream.current().toLowerCase()) ? 'keyword' : 'variableName'
    }

    if (stream.match(/^[=+\-*/^<>!&|%()[\]{},;:]/)) return 'operator'

    stream.next()
    return null
  },

  copyState: (s: State): State => ({ ...s }),
  blankLine: (_state: State) => {},
  indent: () => null,
  languageData: { commentTokens: { line: '//', block: { open: '/*', close: '*/' } } },
})
