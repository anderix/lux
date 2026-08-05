" Vim / Neovim syntax file for lux — highlighting only, no language machinery.
" One regex-based file, works the same in classic Vim and Neovim. Install by
" copying editors/vim/ into ~/.vim/ (Vim) or ~/.config/nvim/ (Neovim).
"
" Written by David M. Anderson with AI assistance. MIT, same as lux.

if exists("b:current_syntax")
  finish
endif

" keywords and control flow
syntax keyword luxKeyword     let var if else while for in func return struct enum match

" the boolean values
syntax keyword luxBoolean     true false

" the built-in types
syntax keyword luxType        int float bool string Option Result Output

" the Option and Result values, spelled lowercase in lux
syntax keyword luxConstructor some none ok err

" the built-in functions
syntax keyword luxBuiltin     print eprint input readLine readFile writeFile args run length contains replace split parseInt parseFloat

" comments run // to end of line
syntax match   luxComment     "//.*$" contains=@Spell

" strings are single-line; the four escapes lux understands are \n \t \" \\
syntax match   luxEscape      "\\[nt\"\\]" contained
syntax region  luxString      start=+"+ end=+"+ oneline contains=luxEscape

" numbers (float tried first so 3.0 wins over a bare 3)
syntax match   luxFloat       "\<\d\+\.\d\+\>"
syntax match   luxNumber      "\<\d\+\>"

" Link each group to a standard role. The colours come from whatever colorscheme
" is active, so nothing here hard-codes a colour.
highlight default link luxKeyword     Keyword
highlight default link luxBoolean     Boolean
highlight default link luxType        Type
highlight default link luxConstructor Constant
highlight default link luxBuiltin     Function
highlight default link luxComment     Comment
highlight default link luxString      String
highlight default link luxEscape      SpecialChar
highlight default link luxFloat       Float
highlight default link luxNumber      Number

let b:current_syntax = "lux"
