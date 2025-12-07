" Vim tooling for wetsocks project
" Last Change: 2025 Dec 06
" Maintainer: Siddeshwar <siddeshwar.work@gmail.com>

if exists("g:loaded_wetsocks")
    finish
endif
let g:loaded_wetsocks = 1

function! s:CompilerSetup()
    compiler cargo
    set makeprg=cargo\ check
endfunction
autocmd BufNewFile,BufRead * call s:CompilerSetup()

set path=.,
set path+=static/
set path+=server/src/**
set path+=crypto-wasm/src/**

function! s:TermRunOutput(cmd)
    execute "terminal ++shell ++close bash -ic \"" . a:cmd . "\""
endfunction

command! Test call <sid>TermRunOutput("cargo test;
            \read -p 'Press enter to continue'")

function! s:TermRun(cmd)
    execute "terminal ++shell ++close " . a:cmd
endfunction

command! Format :call <sid>TermRun("cargo fmt")
nnoremap <leader>cf :Format<cr>

command! Run :call <sid>TermRun("cargo run")
nnoremap <f5> :Run<cr>

function! s:DeployWasm()
    let l:cmd = "wasm-pack build crypto-wasm --target web
                \ --out-dir \"" . getcwd() ."/static/crypto-wasm\""
    call <sid>TermRun(l:cmd)
endfunction

command! DeployWasm :call <sid>DeployWasm()
nnoremap <f6> :DeployWasm<cr>
