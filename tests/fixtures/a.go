package fixture

import "fmt"

// Limit is a constant.
const Limit = 3

var counter int

// Server holds state.
type Server struct {
	name string
}

// Handler is an interface.
type Handler interface {
	Serve() error
}

// New makes a Server.
func New(name string) *Server {
	return &Server{name: name}
}

// Serve runs it.
func (s *Server) Serve() error {
	fmt.Println(s.name)
	return nil
}
