use std::collections::HashMap;

pub fn generate_wat_template(functions: &HashMap<String, String>, init_code: &str) -> String {
    format!(
        r#"(module
  (import "console" "log" (func $log (param f64)))

  (tag $exception (param i32))

  (type $HashMapEntry (struct
    (field $key (mut i32))
    (field $value (mut anyref))
  ))

  (type $EntriesArray (array (mut (ref null $HashMapEntry))))

  (type $HashMap (struct
    (field $entries (mut (ref $EntriesArray)))
    (field $size (mut i32))
  ))

  (type $Scope (struct
    (field $parent (mut (ref null $Scope)))
    (field $variables (mut (ref $HashMap)))
    (field $constants (mut (ref $HashMap)))
  ))

  (type $JSArgs (array (mut anyref)))

  (type $JSFunc
    (func 
      (param $scope (ref $Scope))
      (param $arguments (ref null $JSArgs))
      (result anyref)
    )
  )

  (type $Function (struct
    (field $scope (mut (ref $Scope)))
    (field $func (mut (ref $JSFunc)))
  ))

  (type $Object (struct
    (field $properties (mut (ref $HashMap)))
  ))

  (type $Number (struct (field (mut f64))))

  (func $new_hashmap (result (ref $HashMap))
    (struct.new $HashMap
      (array.new $EntriesArray (ref.null $HashMapEntry) (i32.const 10))
      (i32.const 0)
    )
  )

  (func $new_function
    (param $scope (ref $Scope))
    (param $function (ref $JSFunc))
    (result (ref $Function))

    (struct.new $Function
      (local.get $scope)
      (local.get $function)
    )
  )

  (func $new_number (param $number f64) (result (ref $Number))
    (struct.new $Number
      (local.get $number)
    )
  )

  (func $new_scope (param $parent (ref null $Scope)) (result (ref $Scope))
    (struct.new $Scope
      (local.get $parent)
      (call $new_hashmap)
      (call $new_hashmap)
    )
  )

  (func $set_variable (param $scope (ref $Scope)) (param $name i32) (param $value anyref)
    (call $hashmap_set
      (struct.get $Scope $variables (local.get $scope))
      (local.get $name)
      (local.get $value)
    )
  )

  (func $get_variable (param $scope (ref $Scope)) (param $name i32) (result anyref)
    (local $current_scope (ref null $Scope))
    (local $value anyref)

    (local.set $current_scope (local.get $scope))
    (loop $search_loop
      (local.set $value
        (call $hashmap_get
          (struct.get $Scope $variables (local.get $current_scope))
          (local.get $name)
        )
      )
      (if (ref.is_null (local.get $value))
        (then
          (local.set $current_scope (struct.get $Scope $parent (local.get $current_scope)))
          (br $search_loop)
        )
        (else
          (return (local.get $value))
        )
      )
    )
    (return (ref.null any))
  )

  (func $hashmap_set (param $map (ref $HashMap)) (param $key i32) (param $value anyref)
    (local $entries (ref $EntriesArray))
    (local $new_entry (ref $HashMapEntry))
    (local $new_size i32)
    (local $new_entries (ref $EntriesArray))
    (local $i i32)

    (local.set $entries (struct.get $HashMap $entries (local.get $map)))
    (local.set $new_entry (struct.new $HashMapEntry (local.get $key) (local.get $value)))

    (if (i32.ge_u (struct.get $HashMap $size (local.get $map)) (array.len (local.get $entries)))
      (then
        (local.set $new_size (i32.mul (array.len (local.get $entries)) (i32.const 2)))
        (local.set $new_entries (array.new $EntriesArray (ref.null $HashMapEntry) (local.get $new_size)))

        (local.set $i (i32.const 0))
        (loop $copy_loop
          (if (i32.lt_u (local.get $i) (array.len (local.get $entries)))
            (then
              (array.set $EntriesArray (local.get $new_entries) (local.get $i)
                (array.get $EntriesArray (local.get $entries) (local.get $i)))
              (local.set $i (i32.add (local.get $i) (i32.const 1)))
              (br $copy_loop)
            )
          )
        )

        (struct.set $HashMap $entries (local.get $map) (local.get $new_entries))
        (local.set $entries (local.get $new_entries))
      )
    )

    (array.set $EntriesArray (local.get $entries) (struct.get $HashMap $size (local.get $map)) (local.get $new_entry))

    (struct.set $HashMap $size (local.get $map) (i32.add (struct.get $HashMap $size (local.get $map)) (i32.const 1)))
  )

  (func $hashmap_get (param $map (ref $HashMap)) (param $key i32) (result anyref)
    (local $i i32)
    (local $entries (ref $EntriesArray))
    (local.set $entries (struct.get $HashMap $entries (local.get $map)))
    (local.set $i (i32.const 0))
    (loop $search_loop
      (if (i32.lt_u (local.get $i) (struct.get $HashMap $size (local.get $map)))
        (then
          (if (i32.eq
                (struct.get $HashMapEntry $key (array.get $EntriesArray (local.get $entries) (local.get $i)))
                (local.get $key))
            (then
              (return (struct.get $HashMapEntry $value (array.get $EntriesArray (local.get $entries) (local.get $i))))
            )
          )
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br $search_loop)
        )
      )
    )
    (ref.null any)
  )

  (func $call_function (param $scope (ref $Scope)) (param $func anyref) (param $arguments (ref null $JSArgs)) (result anyref)
    (local $function (ref $Function))
    (local $js_func (ref $JSFunc))

    (local.set $function (ref.cast (ref $Function) (local.get $func)))
    (local.set $js_func (struct.get $Function $func (local.get $function)))

    (call_ref $JSFunc
      (struct.get $Function $scope (local.get $function))
      (local.get $arguments)
      (local.get $js_func)
    )
  )

  (func $add (param $arg1 anyref) (param $arg2 anyref) (result anyref)
    (local $num1 (ref $Number))
    (local $num2 (ref $Number))
    (local $result f64)

    (if (i32.and
          (ref.test (ref $Number) (local.get $arg1))
          (ref.test (ref $Number) (local.get $arg2)))
      (then
        (local.set $num1 (ref.cast (ref $Number) (local.get $arg1)))
        (local.set $num2 (ref.cast (ref $Number) (local.get $arg2)))
        (local.set $result
          (f64.add
            (struct.get $Number 0 (local.get $num1))
            (struct.get $Number 0 (local.get $num2))
          )
        )
        (return (call $new_number (local.get $result)))
      )
      (else
        (i32.const 0)
        (throw $exception)
      )
    )
    (ref.null any)
  )

  {init_code}

  (export "init" (func $init))
)"#,
        init_code = init_code
    )
}
