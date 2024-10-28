(module
  (type (func (param i32)))
  (type (func (param anyref)))
  (type $write_type (func (param i32 i32 i32 i32) (result i32)))
  (import "wasi_snapshot_preview1" "fd_write" (func $write (type $write_type)))

  ;; 64KB
  (memory (export "memory") 1)

  (tag $InternalException (type 0))
  (tag $JSException (type 1))

  (global $free_memory_offset i32 (i32.const {free_memory_offset}))

  ;; Types that can be passed as reference types:
  ;; i31ref 0 - false
  ;; i31ref 1 - true
  ;; i31ref 2 - null
  ;; null     - undefined

  ;; define new line in memory
  (data (i32.const 192) "\n")
  ;; define empty string in memory
  (data (i32.const 196) " ")
  {data}

  (type $StaticString (struct
    (field $offset i32)
    (field $length i32)
  ))

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
      (param $this anyref)
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
    (field $prototype (mut anyref))
  ))

  (type $Number (struct (field (mut f64))))

  (func $new_hashmap (result (ref $HashMap))
    (struct.new $HashMap
      (array.new $EntriesArray (ref.null $HashMapEntry) (i32.const 10))
      (i32.const 0)
    )
  )

  (func $new_object (result (ref $Object))
    (struct.new $Object
      (call $new_hashmap)
      (ref.null any)
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

  (func $new_boolean (param $bool i32) (result i31ref)
    (ref.i31 (local.get $bool))
    return
  )

  (func $cast_ref_to_i32_bool (param $arg anyref) (result i32)
    (local $res i32)
    (if (ref.test nullref (local.get $arg))
      (then
        (return (i32.const 0))
      )
    )
    (if (ref.test i31ref (local.get $arg))
      (then
        (i31.get_s (ref.cast (ref null i31) (local.get $arg)))
        (local.set $res)
        (if (i32.eq (local.get $res) (i32.const 1))
          ;; boolean true
          (then
            (return (i32.const 1))
          )
        )
        ;; anything else is false
        (return (i32.const 0))
      )
    )
    (if (ref.test (ref $Number) (local.get $arg))
      (then
        (return (i32.eqz (f64.eq
            (struct.get $Number 0 (ref.cast (ref $Number) (local.get $arg)))
            (f64.const 0)
          ))
        )
      )
    )
    (if (ref.test (ref $StaticString) (local.get $arg))
      (then
        (return (i32.const 1))
      )
    )
 
    i32.const 0
  )

  (func $new_string (param $offset i32) (param $length i32) (result (ref $StaticString))
    (struct.new $StaticString
      (local.get $offset)
      (local.get $length)
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

  (func $get_property (param $target anyref) (param $name i32) (result anyref)
    ;; for now we just support $Object for properties
    (if (ref.test (ref $Object) (local.get $target))
      (then
        (call $hashmap_get
          (struct.get $Object $properties (ref.cast (ref $Object) (local.get $target)))
          (local.get $name)
        )
        (return)
      )
    )

    (throw $InternalException (i32.const 0))
  )

  (func $set_property (param $target anyref) (param $name i32) (param $value anyref)
    ;; for now we just support $Object for properties
    (if (ref.test (ref $Object) (local.get $target))
      (then
        (call $hashmap_set
          (struct.get $Object $properties (ref.cast (ref $Object) (local.get $target)))
          (local.get $name)
          (local.get $value)
        )
        (return)
      )
    )

    (throw $InternalException (i32.const 1))
  )

  (func $hashmap_set (param $map (ref $HashMap)) (param $key i32) (param $value anyref)
    (local $entries (ref $EntriesArray))
    (local $new_entry (ref $HashMapEntry))
    (local $new_size i32)
    (local $new_entries (ref $EntriesArray))
    (local $i i32)
    (local $found i32)  ;; New local to track if we found the key

    (local.set $entries (struct.get $HashMap $entries (local.get $map)))
    (local.set $new_entry (struct.new $HashMapEntry (local.get $key) (local.get $value)))
    (local.set $found (i32.const 0))  ;; Initialize found flag to false

    ;; First, search for existing key
    (local.set $i (i32.const 0))
    (loop $search_loop
      (if (i32.lt_u (local.get $i) (struct.get $HashMap $size (local.get $map)))
        (then
          (if (i32.eq
                (struct.get $HashMapEntry $key (array.get $EntriesArray (local.get $entries) (local.get $i)))
                (local.get $key))
            (then
              ;; Key found - update the value
              (array.set $EntriesArray (local.get $entries) (local.get $i) (local.get $new_entry))
              (local.set $found (i32.const 1))  ;; Set found flag to true
            )
          )
          (if (i32.eqz (local.get $found))  ;; Only continue searching if not found
            (then
              (local.set $i (i32.add (local.get $i) (i32.const 1)))
              (br $search_loop)
            )
          )
        )
      )
    )

    ;; If key wasn't found, proceed with insertion
    (if (i32.eqz (local.get $found))
      (then
        ;; Check if we need to resize
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

        ;; Add new entry and increment size
        (array.set $EntriesArray (local.get $entries) (struct.get $HashMap $size (local.get $map)) (local.get $new_entry))
        (struct.set $HashMap $size (local.get $map) (i32.add (struct.get $HashMap $size (local.get $map)) (i32.const 1)))
      )
    )
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

  (func $call_function (param $scope (ref $Scope)) (param $func anyref) (param $this anyref) (param $arguments (ref null $JSArgs)) (result anyref)
    (local $function (ref $Function))
    (local $js_func (ref $JSFunc))

    (local.set $function (ref.cast (ref $Function) (local.get $func)))
    (local.set $js_func (struct.get $Function $func (local.get $function)))

    (call_ref $JSFunc
      (struct.get $Function $scope (local.get $function))
      (local.get $this)
      (local.get $arguments)
      (local.get $js_func)
    )
  )

  (func $add (param $arg1 anyref) (param $arg2 anyref) (result anyref)
    (local $num1 (ref $Number))
    (local $num2 (ref $Number))
    (local $result f64)

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

  (func $sub (param $arg1 anyref) (param $arg2 anyref) (result anyref)
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
          (f64.sub
            (struct.get $Number 0 (local.get $num1))
            (struct.get $Number 0 (local.get $num2))
          )
        )
        (return (call $new_number (local.get $result)))
      )
    )
    (ref.null any)
  )

  (func $strict_not_equal (param $arg1 anyref) (param $arg2 anyref) (result i31ref)
    (return 
      (ref.i31
        (i32.eqz
          (i31.get_s
            (ref.cast 
              (ref null i31)
              (call $strict_equal (local.get $arg1) (local.get $arg2))))))
    )
  )
  (func $strict_equal (param $arg1 anyref) (param $arg2 anyref) (result i31ref)
    (local $num1 (ref $Number))
    (local $num2 (ref $Number))
    (local $result f64)

    (if (i32.and
          (ref.test (ref $Number) (local.get $arg1))
          (ref.test (ref $Number) (local.get $arg2)))
      (then
        (local.set $num1 (ref.cast (ref $Number) (local.get $arg1)))
        (local.set $num2 (ref.cast (ref $Number) (local.get $arg2)))
        (return 
          (ref.i31 (f64.eq
            (struct.get $Number 0 (local.get $num1))
            (struct.get $Number 0 (local.get $num2))
          ))
        )
      )
    )

    ;; if both args are null, return true
    (if (i32.and
          (ref.test nullref (local.get $arg1))
          (ref.test nullref (local.get $arg1)))
      (then
        (return (ref.i31 (i32.const 1)))
      )
    )
    ;; if both args are bool or null and are equal, return true
    (if (i32.and
          (ref.test i31ref (local.get $arg1))
          (ref.test i31ref (local.get $arg2)))
      (then
        (return (ref.i31 (i32.eq
          (i31.get_s (ref.cast (ref null i31) (local.get $arg1)))
          (i31.get_s (ref.cast (ref null i31) (local.get $arg2)))
        )))
      )
    )

    (ref.i31 (i32.const 0))
  )

  (func $less_than (param $arg1 anyref) (param $arg2 anyref) (result i31ref)
    (local $num1 (ref $Number))
    (local $num2 (ref $Number))
    (local $result f64)

    (if (i32.and
          (ref.test (ref $Number) (local.get $arg1))
          (ref.test (ref $Number) (local.get $arg2)))
      (then
        (local.set $num1 (ref.cast (ref $Number) (local.get $arg1)))
        (local.set $num2 (ref.cast (ref $Number) (local.get $arg2)))
        (return 
          (ref.i31 (f64.lt
            (struct.get $Number 0 (local.get $num1))
            (struct.get $Number 0 (local.get $num2))
          ))
        )
      )
    )

    (ref.i31 (i32.const 0))
  )

  (func $greater_than_or_equal (param $arg1 anyref) (param $arg2 anyref) (result i32)
    (local $num1 (ref $Number))
    (local $num2 (ref $Number))
    (local $result f64)

    (if (i32.and
          (ref.test (ref $Number) (local.get $arg1))
          (ref.test (ref $Number) (local.get $arg2)))
      (then
        (local.set $num1 (ref.cast (ref $Number) (local.get $arg1)))
        (local.set $num2 (ref.cast (ref $Number) (local.get $arg2)))
        (return 
          (f64.ge
            (struct.get $Number 0 (local.get $num1))
            (struct.get $Number 0 (local.get $num2))
          )
        )
      )
    )

    (i32.const 0)
  )

  (func $increment_number (param $arg1 anyref) (result anyref)
    (local $num (ref $Number))
    (local $result f64)

    (if (ref.test (ref $Number) (local.get $arg1))
      (then
        (local.set $num (ref.cast (ref $Number) (local.get $arg1)))
        (local.set $result
          (f64.add
            (struct.get $Number 0 (local.get $num))
            (f64.const 1)
          )
        )
        (return (call $new_number (local.get $result)))
      )
    )
    (ref.null any)
  )

  ;; TODO: can we update in-place?
  (func $decrement_number (param $arg1 anyref) (result anyref)
    (local $num (ref $Number))
    (local $result f64)

    (if (ref.test (ref $Number) (local.get $arg1))
      (then
        (local.set $num (ref.cast (ref $Number) (local.get $arg1)))
        (local.set $result
          (f64.sub
            (struct.get $Number 0 (local.get $num))
            (f64.const 1)
          )
        )
        (return (call $new_number (local.get $result)))
      )
    )
    (ref.null any)
  )

  ;; TODO: fix this
(func $writeF64AsAscii (param $value f64) (param $offset i32) (result i32)
  (local $isNegative i32)
  (local $intPart i64)
  (local $currentOffset i32)
  (local $digitCount i32)
  (local $fracValue f64)
  
  ;; Initialize current offset
  (local.set $currentOffset (local.get $offset))
  
  ;; Handle negative numbers
  (local.set $isNegative 
    (i32.lt_s 
      (i32.wrap_i64 
        (i64.shr_u 
          (i64.reinterpret_f64 (local.get $value))
          (i64.const 63)
        )
      )
      (i32.const 0)
    )
  )
  
  ;; Write minus sign if negative
  (if (local.get $isNegative)
    (then
      ;; Write '-' (ASCII 45)
      (i32.store8 (local.get $currentOffset) (i32.const 45))
      (local.set $currentOffset (i32.add (local.get $currentOffset) (i32.const 1)))
      ;; Make value positive
      (local.set $value (f64.neg (local.get $value)))
    )
  )
  
  ;; Get integer part
  (local.set $intPart (i64.trunc_f64_s (local.get $value)))
  
  ;; Write integer part digits in reverse
  (local.set $digitCount (i32.const 0))
  (block $break
    (loop $digit_loop
      ;; Write current digit
      (i32.store8 
        (i32.add 
          (local.get $currentOffset) 
          (local.get $digitCount)
        )
        (i32.add
          (i32.wrap_i64 
            (i64.rem_u 
              (local.get $intPart)
              (i64.const 10)
            )
          )
          (i32.const 48)  ;; ASCII '0'
        )
      )
      (local.set $digitCount (i32.add (local.get $digitCount) (i32.const 1)))
      
      ;; Update intPart
      (local.set $intPart (i64.div_u (local.get $intPart) (i64.const 10)))
      
      ;; Continue if intPart > 0
      (br_if $digit_loop (i64.gt_s (local.get $intPart) (i64.const 0)))
    )
  )
  
  ;; Reverse the digits we just wrote
  (local.set $currentOffset 
    (call $reverseBytes 
      (local.get $currentOffset)
      (local.get $digitCount)
    )
  )
  
  ;; Get fractional part
  (local.set $fracValue (f64.sub 
    (local.get $value)
    (f64.convert_i64_s (i64.trunc_f64_s (local.get $value)))
  ))
  
  ;; Only write decimal point and fraction if there is a fractional part
  (if (f64.gt (local.get $fracValue) (f64.const 0))
    (then
      ;; Add decimal point
      (i32.store8 (local.get $currentOffset) (i32.const 46))  ;; ASCII '.'
      (local.set $currentOffset (i32.add (local.get $currentOffset) (i32.const 1)))
      
      ;; Write fractional digits until we hit zero or max precision
      (local.set $digitCount (i32.const 0))
      (block $frac_break
        (loop $frac_loop
          ;; Break if we've written 16 digits (typical f64 precision limit)
          (br_if $frac_break (i32.ge_u (local.get $digitCount) (i32.const 16)))
          
          ;; Multiply by 10 to get next digit
          (local.set $fracValue (f64.mul (local.get $fracValue) (f64.const 10)))
          
          ;; Get the digit
          (local.set $intPart (i64.trunc_f64_s (local.get $fracValue)))
          
          ;; Write the digit
          (i32.store8 
            (local.get $currentOffset)
            (i32.add
              (i32.wrap_i64 (local.get $intPart))
              (i32.const 48)  ;; ASCII '0'
            )
          )
          
          ;; Update fracValue
          (local.set $fracValue 
            (f64.sub 
              (local.get $fracValue)
              (f64.convert_i64_s (local.get $intPart))
            )
          )
          
          ;; Update counters
          (local.set $currentOffset (i32.add (local.get $currentOffset) (i32.const 1)))
          (local.set $digitCount (i32.add (local.get $digitCount) (i32.const 1)))
          
          ;; Continue if we have more significant digits
          (br_if $frac_loop (f64.gt (local.get $fracValue) (f64.const 0)))
        )
      )
    )
  )
  
  ;; Return number of bytes written
  (i32.sub (local.get $currentOffset) (local.get $offset))
)

;; Helper function to reverse bytes (same as before)
(func $reverseBytes (param $start i32) (param $length i32) (result i32)
  (local $left i32)
  (local $right i32)
  (local $temp i32)
  
  (local.set $left (local.get $start))
  (local.set $right (i32.sub 
    (i32.add (local.get $start) (local.get $length))
    (i32.const 1)
  ))
  
  (block $break
    (loop $reverse_loop
      (br_if $break (i32.ge_u (local.get $left) (local.get $right)))
      
      ;; Swap bytes
      (local.set $temp 
        (i32.load8_u (local.get $left))
      )
      (i32.store8 
        (local.get $left)
        (i32.load8_u (local.get $right))
      )
      (i32.store8 
        (local.get $right)
        (local.get $temp)
      )
      
      ;; Update pointers
      (local.set $left (i32.add (local.get $left) (i32.const 1)))
      (local.set $right (i32.sub (local.get $right) (i32.const 1)))
      (br $reverse_loop)
    )
  )
  
  (i32.add (local.get $start) (local.get $length))
)

  (func $log (param $arguments (ref null $JSArgs))
    (local $i i32)                    ;; loop counter
    (local $len i32)                  ;; length of arguments array
    (local $current anyref)           ;; current argument being processed
    (local $offset i32)               ;; current memory offset for data
    (local $iovectors_offset i32)     ;; offset for iovectors
    (local $written_length i32)       ;; length of written data
    (local $num_val f64)              ;; temporary storage for number value
    (local $str_ref (ref $StaticString))  ;; temporary storage for string reference
    (local $num_ref (ref $Number))    ;; temporary storage for number reference
    (local $handled i32)
 
    ;; Get length of arguments array
    (local.set $len 
      (array.len (ref.cast (ref $JSArgs) (local.get $arguments)))
    )
 
    ;; Set initial offsets
    (local.set $iovectors_offset 
      (global.get $free_memory_offset)
    )
    ;; set offset for iovectors. we need place for arguments * 2
    ;; vectors, one for each argument, one for a space after each
    ;; arguments and one for a new line at the end
    (local.set $offset 
      (i32.add 
        (global.get $free_memory_offset)
        (i32.mul (local.get $len) (i32.const 16))
      )
    )
    
    ;; Initialize loop counter
    (local.set $i (i32.const 0))
    
    ;; Main loop through arguments
    (loop $process_args
      (block $process_arg
        ;; Get current argument
        (local.set $current 
          (array.get $JSArgs
            (ref.cast (ref $JSArgs) (local.get $arguments))
            (local.get $i)
          )
        )

        ;; argument not handled yet
        (local.set $handled (i32.const 0))
       
        ;; Check if it's a Number
        (if (ref.test (ref $Number) (local.get $current))
          (then
            ;; Cast to Number and get value
            (local.set $num_ref 
              (ref.cast (ref $Number) (local.get $current))
            )
            (local.set $num_val 
              (struct.get $Number 0 (local.get $num_ref))
            )
            
            ;; Write number to memory and get length
            (local.set $written_length
              (call $writeF64AsAscii 
                (local.get $num_val)
                (local.get $offset)
              )
            )
            
            ;; Store iovector data
            (i32.store (local.get $iovectors_offset) (local.get $offset))
            (i32.store 
              (i32.add (local.get $iovectors_offset) (i32.const 4))
              (local.get $written_length)
            )
            
            ;; Update offsets
            (local.set $offset 
              (i32.add (local.get $offset) (local.get $written_length))
            )
            (local.set $iovectors_offset 
              (i32.add (local.get $iovectors_offset) (i32.const 8))
            )
            
            ;; argument handled
            (local.set $handled (i32.const 1))
          )
        )
        
        ;; Check if it's a StaticString
        (if (ref.test (ref $StaticString) (local.get $current))
          (then
            ;; Cast to StaticString
            (local.set $str_ref 
              (ref.cast (ref $StaticString) (local.get $current))
            )
            
            ;; Store iovector data
            (i32.store 
              (local.get $iovectors_offset)
              (struct.get $StaticString $offset (local.get $str_ref))
            )
            (i32.store 
              (i32.add (local.get $iovectors_offset) (i32.const 4))
              (struct.get $StaticString $length (local.get $str_ref))
            )
            
            ;; Update offset for next write
            (local.set $offset 
              (i32.add 
                (local.get $offset)
                (struct.get $StaticString $length (local.get $str_ref))
              )
            )
            (local.set $iovectors_offset 
              (i32.add (local.get $iovectors_offset) (i32.const 8))
            )
    
            ;; argument handled
            (local.set $handled (i32.const 1))
          )
        )

        ;; Increment counter
        (local.set $i (i32.add (local.get $i) (i32.const 1)))

        ;; Check if we're done processing arguments
        (br_if $process_arg
          (i32.ge_u (local.get $i) (local.get $len))
        )

        ;; after each argument, but the last, we put in a space
        (i32.store (local.get $iovectors_offset) (i32.const 196))
        (i32.store 
          (i32.add (local.get $iovectors_offset) (i32.const 4))
          (i32.const 1)
        )
 
        (local.set $iovectors_offset 
          (i32.add (local.get $iovectors_offset) (i32.const 8))
        )
 
        (br $process_args)
      ) 
    )

    ;; put newline at the end
    (i32.store (local.get $iovectors_offset) (i32.const 192))
    (i32.store 
      (i32.add (local.get $iovectors_offset) (i32.const 4))
      (i32.const 1)
    )
    
    ;; Call write with all accumulated iovectors
    (call $write 
      (i32.const 1)  ;; stdout
      (global.get $free_memory_offset)  ;; iovectors start
      (i32.mul (local.get $len) (i32.const 2))  ;; number of iovectors
      (i32.const 50)  ;; where to write result
    )
    drop
  )

  {init_code}

  (export "_start" (func $init))
)
