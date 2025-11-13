;; Simple WASM test module that demonstrates host function calls
(module
  ;; Import host functions
  (import "env" "host_add" (func $host_add (param i32 i32) (result i32)))
  (import "env" "host_log" (func $host_log (param i32 i32)))

  ;; Memory (1 page = 64KB)
  (memory (export "memory") 1)

  ;; Store "Hello from WASM!" at memory offset 0
  (data (i32.const 0) "Hello from WASM!")

  ;; Main entry point
  (func (export "main") (result i32)
    ;; Log the hello message
    ;; host_log(ptr=0, len=16)
    (call $host_log (i32.const 0) (i32.const 16))

    ;; Test addition: 10 + 32 = 42
    (call $host_add (i32.const 10) (i32.const 32))
  )
)
