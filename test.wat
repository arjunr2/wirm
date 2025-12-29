(component $PARENT
  (type $t (func (result string)))
  (component
    (import "a" (func (type $t)))
  )
  (component
    (alias outer $PARENT $t (type $my_type))
    (alias outer 0 $my_type (type $my_type_again))
    (import "a" (func (type $my_type_again)))
  )
)