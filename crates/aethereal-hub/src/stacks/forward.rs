//! What each of the hub's stacks forwards to its `Serving`: every handler but its listen's answer, which each stack
//! decides on itself. The paths are written whole, so a stack's module brings in only what it names itself.

/// The stack `$stack`, whose layers are `$layers`, whose actor layers pass on each of `$inward`, and which depends on
/// TCP and on each of `$depends`, its actor layers and its handlers, as it finds each at its typed path: a root, which
/// begins by finding its layers and listening, and each of its handlers of HTTP's and TCP's kinds, of its actor layers'
/// answers, and of its body streams' kinds, one call into its `Serving`; but its listen's answer, which each stack
/// decides on itself.
macro_rules! stack {
    ($stack:ident, $layers:ty, [$($inward:ty),*], depends($($depends:ty),*)) => {
        impl ::aethereal_actor::ChildOf for $stack {
            type Parent = ::aethereal_actor::Top;
        }

        impl ::aethereal_actor::Actor for $stack {
            type Config = ::aethereal_http::stack::StackSettings;
            type Key = ();
            type Handles = (
                ::aethereal_actor::Answer<::aethereal_tcp::Listening, ::aethereal_http::stack::Binding>,
                ::aethereal_actor::Part<::aethereal_tcp::Accepting>,
                ::aethereal_actor::Answer<::aethereal_tcp::Unbound, ::aethereal_actor::Number>,
                ::aethereal_actor::Part<::aethereal_tcp::Reading>,
                ::aethereal_actor::Answer<::aethereal_tcp::HungUp, ::aethereal_actor::Number>,
                ::aethereal_actor::Answer<::aethereal_tcp::Wrote, ::aethereal_http::stack::Sending>,
                ::aethereal_actor::Answer<::aethereal_http::stack::Response, ::aethereal_http::stack::Answering>,
                ::aethereal_actor::Answer<::aethereal_actor::Slept, ::aethereal_http::stack::Deadline>,
                ::aethereal_http::stack::Gather,
                ::aethereal_tcp::Shut,
                ::aethereal_actor::Answer<::aethereal_http::stack::Outward, ::aethereal_http::stack::ParkedResponse>,
                ::aethereal_actor::Answer<::aethereal_http::stack::PartStep, ::aethereal_http::stack::ParkedPart>,
                ::aethereal_actor::Open<::aethereal_http::stack::InboundBody>,
                ::aethereal_actor::More,
                ::aethereal_actor::Enough,
                ::aethereal_actor::Ended,
                ::aethereal_actor::Part<::aethereal_http::stack::OutboundBody>,
                ::aethereal_actor::Answer<::aethereal_http::stack::SourceEnd, ::aethereal_actor::Number>,
                $(::aethereal_actor::Answer<::aethereal_http::stack::Inward<$inward>, ::aethereal_http::stack::Parked>,)*
            );
            type Depends = (::aethereal_tcp::Tcp, $($depends,)*);
            type Spawns = ();

            fn begin(
                settings: ::aethereal_http::stack::StackSettings,
                turn: &mut ::aethereal_actor::Beginning<'_, Self>,
            ) -> Result<Self, ::aethereal_actor::Fault> {
                let layers: ::aethereal_http::stack::Layers<$layers> = ::aethereal_http::stack::layers(turn)?;

                Ok(Self { serving: ::aethereal_http::stack::Serving::listen(turn, &settings, layers)? })
            }

            fn end(self, _: &mut ::aethereal_actor::Ending<'_, Self>) -> Result<(), ::aethereal_actor::Fault> {
                Ok(())
            }
        }

        impl ::aethereal_http::stack::Stack for $stack {}

        impl ::aethereal_actor::Handles<::aethereal_actor::Part<::aethereal_tcp::Accepting>> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ();

            fn handle(
                &mut self,
                part: ::aethereal_actor::Part<::aethereal_tcp::Accepting>,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.accept(part, turn)
            }
        }

        impl ::aethereal_actor::Receives<::aethereal_actor::Answer<::aethereal_tcp::Unbound, ::aethereal_actor::Number>>
            for $stack
        {
            fn handle(
                &mut self,
                unbound: (::aethereal_actor::Answered<::aethereal_tcp::Unbound>, ::aethereal_actor::Number),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.unbound(unbound, turn)
            }
        }

        impl ::aethereal_actor::Handles<::aethereal_actor::Part<::aethereal_tcp::Reading>> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ();

            fn handle(
                &mut self,
                part: ::aethereal_actor::Part<::aethereal_tcp::Reading>,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.read(part, turn)
            }
        }

        impl ::aethereal_actor::Receives<::aethereal_actor::Answer<::aethereal_tcp::HungUp, ::aethereal_actor::Number>>
            for $stack
        {
            fn handle(
                &mut self,
                hung_up: (::aethereal_actor::Answered<::aethereal_tcp::HungUp>, ::aethereal_actor::Number),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.hung_up(hung_up, turn)
            }
        }

        impl ::aethereal_actor::Receives<
            ::aethereal_actor::Answer<::aethereal_tcp::Wrote, ::aethereal_http::stack::Sending>,
        > for $stack
        {
            fn handle(
                &mut self,
                wrote: (::aethereal_actor::Answered<::aethereal_tcp::Wrote>, ::aethereal_http::stack::Sending),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.wrote(wrote, turn)
            }
        }

        impl ::aethereal_actor::Receives<
            ::aethereal_actor::Answer<::aethereal_http::stack::Response, ::aethereal_http::stack::Answering>,
        > for $stack
        {
            fn handle(
                &mut self,
                answered: (
                    ::aethereal_actor::Answered<::aethereal_http::stack::Response>,
                    ::aethereal_http::stack::Answering,
                ),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.answered(answered, turn)
            }
        }

        impl ::aethereal_actor::Receives<
            ::aethereal_actor::Answer<::aethereal_actor::Slept, ::aethereal_http::stack::Deadline>,
        > for $stack
        {
            fn handle(
                &mut self,
                slept: (::aethereal_actor::Answered<::aethereal_actor::Slept>, ::aethereal_http::stack::Deadline),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.slept(slept, turn)
            }
        }

        impl ::aethereal_actor::Handles<::aethereal_http::stack::Gather> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ::aethereal_actor::Reply<::aethereal_http::stack::Gathered>;

            fn handle(
                &mut self,
                gather: ::aethereal_http::stack::Gather,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<::aethereal_actor::Reply<::aethereal_http::stack::Gathered>, ::aethereal_actor::Fault> {
                self.serving.gather(gather, turn)
            }
        }

        impl ::aethereal_actor::Handles<::aethereal_tcp::Shut> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ();

            fn handle(
                &mut self,
                _: ::aethereal_tcp::Shut,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.shut(turn)
            }
        }

        impl ::aethereal_actor::Receives<
            ::aethereal_actor::Answer<::aethereal_http::stack::Outward, ::aethereal_http::stack::ParkedResponse>,
        > for $stack
        {
            fn handle(
                &mut self,
                returned: (
                    ::aethereal_actor::Answered<::aethereal_http::stack::Outward>,
                    ::aethereal_http::stack::ParkedResponse,
                ),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.returned(returned, turn)
            }
        }

        impl ::aethereal_actor::Receives<
            ::aethereal_actor::Answer<::aethereal_http::stack::PartStep, ::aethereal_http::stack::ParkedPart>,
        > for $stack
        {
            fn handle(
                &mut self,
                passed: (
                    ::aethereal_actor::Answered<::aethereal_http::stack::PartStep>,
                    ::aethereal_http::stack::ParkedPart,
                ),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.passed(passed, turn)
            }
        }

        impl ::aethereal_actor::Handles<::aethereal_actor::Open<::aethereal_http::stack::InboundBody>> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ::aethereal_actor::Later<::aethereal_http::stack::BodyRead>;

            fn handle(
                &mut self,
                open: ::aethereal_actor::Open<::aethereal_http::stack::InboundBody>,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<::aethereal_actor::Later<::aethereal_http::stack::BodyRead>, ::aethereal_actor::Fault> {
                self.serving.open_body(open, turn)
            }
        }

        impl ::aethereal_actor::Handles<::aethereal_actor::More> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ();

            fn handle(
                &mut self,
                more: ::aethereal_actor::More,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.grant_body(&more, turn)
            }
        }

        impl ::aethereal_actor::Handles<::aethereal_actor::Enough> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ();

            fn handle(
                &mut self,
                enough: ::aethereal_actor::Enough,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.stop_reading(&enough, turn)
            }
        }

        impl ::aethereal_actor::Receives<::aethereal_actor::Ended> for $stack {
            fn handle(
                &mut self,
                ended: ::aethereal_actor::Ended,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.lose_reader(&ended, turn)
            }
        }

        impl ::aethereal_actor::Handles<::aethereal_actor::Part<::aethereal_http::stack::OutboundBody>> for $stack {
            type From = ::aethereal_actor::Anyone;
            type Returns = ();

            fn handle(
                &mut self,
                part: ::aethereal_actor::Part<::aethereal_http::stack::OutboundBody>,
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.take_part(part, turn)
            }
        }

        impl ::aethereal_actor::Receives<
            ::aethereal_actor::Answer<::aethereal_http::stack::SourceEnd, ::aethereal_actor::Number>,
        > for $stack
        {
            fn handle(
                &mut self,
                ended: (::aethereal_actor::Answered<::aethereal_http::stack::SourceEnd>, ::aethereal_actor::Number),
                turn: &mut ::aethereal_actor::Turn<'_, Self>,
            ) -> Result<(), ::aethereal_actor::Fault> {
                self.serving.end_source(ended, turn)
            }
        }

        $(
            impl ::aethereal_actor::Receives<
                ::aethereal_actor::Answer<::aethereal_http::stack::Inward<$inward>, ::aethereal_http::stack::Parked>,
            > for $stack
            {
                fn handle(
                    &mut self,
                    entered: (
                        ::aethereal_actor::Answered<::aethereal_http::stack::Inward<$inward>>,
                        ::aethereal_http::stack::Parked,
                    ),
                    turn: &mut ::aethereal_actor::Turn<'_, Self>,
                ) -> Result<(), ::aethereal_actor::Fault> {
                    self.serving.entered(entered, turn)
                }
            }
        )*
    };
}
