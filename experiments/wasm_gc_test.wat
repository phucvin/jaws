;;;;(module
;;  (import "console" "log" (func $log (param i32)))
;;  ;; import tag that will be referred to here as $tagname
;;  (tag $tagname (param i32))
;;
;;  ;; Exported function "run" that calls $throwException
;;  (func (export "start")
;;    (try_table $tagname
;;      (call $log (i32.const 1))
;;      i32.const 42
;;      throw $tagname
;;
;;      (catch $tagname
;;        (call $log (i32.const 2))
;;      )
;;      )
;;    end
;;  )
;;)


(module
;;  (import "wasm:js-string" "encodeStringToMemoryUTF16" (func (param i32)))
 ;; (type $char-array (array (mut i16)))

;;  (import "wasm:js-string" "fromCharCodeArray" 
;;    (func $fromCharCodeArray 
;;      (param $str (ref null $char-array)) 
;;      (param $start i32)
;;      (param $end i32)
;;      (result (ref extern))
;;    )
;;  )

;;  (global $hey stringref (string.const "Hey"))
  ;;(import "console" "log" (func $log (param i32)))
  (type (func (param anyref)))
  (type (func (param i32)))
  ;;(tag (import "m" "t") (type 0))
  (tag $exn (type 0))
  (tag (type 1))
  (func $check-throw
    ref.null any
    throw 0
  )
  ;; Define a function that takes an externref and tries to cast it to $custom_type
  (func $check-try-catch-rethrow
    try
      call $check-throw
      unreachable
    catch 0
      ;; the exception arguments are on the stack at this point
      drop
    catch 1
      drop
      ;;i64.const 2
    catch_all
      rethrow 0
    end
  )
  (func $try-with-params
    i32.const 0
    try (param i32) (result i32 i64)
      i32.popcnt
      drop
      call $check-throw
      unreachable
    catch 1
      i64.const 2
    catch_all
      ;;(call $log (i32.const 888))
      i32.const 0
      i64.const 2
    end
    drop
    drop
  )
  (func $mix-old-and-new
    try_table
      try
      catch_all
      end
    end
  )

  (func (export "_start")
    call $try-with-params
  )
)

