# hprof-cat
Experimental Analyzer For Java HPROFs

## Description and Goals

This is more of an experiment and should not be used seriously. My objectives here are
[1] to learn Rust and [2] write something useful enough that can be used as the basis
for future projects (in this case a utility that parses and analyzes Java heap dumps).

Specifically the goal here is to make something that will replace my `libhprof` library
(written in C) which was to be plugged into the `sdb` debugger. The new implementation in
Rust should be at least equally fast as `libhprof` and more complete in terms of the
HPROF entries it can analyze.

Once the above is achieved, I'll be archiving this repo and create a proper library from
the code here with potential Python3 bindings so that it can be plugged in to `sdb`.
