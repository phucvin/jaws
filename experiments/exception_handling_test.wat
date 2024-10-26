(module
  (tag $exn (param i32))
  (func $throw_exn (param i32)
    local.get 0
    throw $exn
  )
  (func $catch_exn (result i32)
    (try_table (result i32)
        i32.const 42
        call $throw_ref_example
        i32.const 0  ;; This won't be reached
      (catch $exn
        drop  ;; Drop the caught exception value
        i32.const 1  ;; Return 1 to indicate the exception was caught
      )
    )
  )
  (func $throw_ref_example (param $exn exnref)
    local.get $exn
    throw_ref
  )
)
