(module
  ;; Import the JavaScript console.log function
  (import "console" "log" (func $log (param i32)))

  ;; Define two custom types
  (type $type_a (struct (field i32)))
  (type $type_b (struct (field f32)))

  ;; Function to create an instance of type_a
  (func $create_type_a (result (ref $type_a))
    (struct.new $type_a (i32.const 42))
  )

  ;; Function to create an instance of type_b
  (func $create_type_b (result (ref $type_b))
    (struct.new $type_b (f32.const 3.14))
  )

  (type $JSArgs (array (mut anyref)))

  ;; Function that takes an anyref and checks its type
  (func $check_type (param $input (ref null $JSArgs))
    (local $i i32)
    (local $len i32)
    (local $current anyref)

    (if (ref.is_null (local.get $input))
      (then
        return
      )
    )

    (local.set $len (array.len (local.get $input)))
    (local.set $i (i32.const 0))

    (call $log (local.get $len))
    (call $log (local.get $i))

    (loop $continue (block $break
      (br_if $break (i32.eq (local.get $i) (local.get $len)))

      (local.set $current 
        (array.get $JSArgs (local.get $input) (local.get $i))
      )

      (local.set $i (i32.add (local.get $i) (i32.const 1)))

      (if (ref.test (ref $type_a) (local.get $current))
        (then
          ;; It's type_a
          (call $log (i32.const 1))
          (br $continue)
        )
      )
      (if (ref.test (ref $type_b) (local.get $current))
        (then
          ;; It's type_b
          (call $log (i32.const 2))
          (br $continue)
        )
      )
      (if (ref.test nullref (local.get $current))
        (then
          ;; It's undefined
          (call $log (i32.const 4))
          (br $continue)
        )
      )
      (if (ref.test i31ref (local.get $current))
        (then
          ;; It's undefined
          (call $log (i31.get_u (ref.cast (ref null i31) (local.get $current))))
          (br $continue)
        )
      )

      ;; It's neither type_a nor type_b
      (call $log (i32.const 0))

      (br $continue)
    ) );; endloop $continue, endblock $break
  )

  ;; Start function to demonstrate usage
  (func $start
    (local $args (ref $JSArgs))

    (local $instance_a (ref $type_a))
    (local $instance_b (ref $type_b))

    ;; Create instances
    (local.set $instance_a (call $create_type_a))
    (local.set $instance_b (call $create_type_b))

    (local.set $args
      (array.new_default $JSArgs
        (i32.const 4) ;; Length of array
      )
    )

    (array.set $JSArgs (local.get $args) (i32.const 0) (local.get $instance_a))
    (array.set $JSArgs (local.get $args) (i32.const 1) (local.get $instance_b))
    (array.set $JSArgs (local.get $args) (i32.const 2) (ref.null any))
    (array.set $JSArgs (local.get $args) (i32.const 3) (ref.i31 (i32.const 0)))
    
    (call $check_type (local.get $args))
    ;; Check types
;;    (call $check_type )
;;    drop
;;    (call $check_type )
;;    drop
;;    (call $check_type (ref.null any))
;;    drop
;;    (call $check_type )
;;    drop
  )

  ;; Export functions
  (export "checkType" (func $check_type))
  (export "start" (func $start))
)
