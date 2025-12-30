(component
  (component $C1
    (type $X' (resource (rep i32)))
    (export $X "X" (type $X'))

    (core func $f (canon resource.drop $X))
    (func (export "f") (param "X" (own $X)) (canon lift (core func $f)))
  )
  (instance $c1 (instantiate $C1))

  (component $C2
    (import "X" (type $X (sub resource)))
    (import "f" (func (param "X" (own $X))))
  )
  (instance $c2 (instantiate $C2
    (with "X" (type $c1 "X"))
    (with "f" (func $c1 "f"))
  ))
)