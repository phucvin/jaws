(func $append (param $a stringref) (param $b stringref)
              (result stringref)
  local.get $a
  local.get $b
  string.concat)
