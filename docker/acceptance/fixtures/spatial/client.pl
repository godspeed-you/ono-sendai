#!/usr/bin/perl
# The client half of the connection fixture of v0.4 §43.3 ("one client/server connection").
# It connects to the fixture listener and holds the connection open, so an established
# connection with two real endpoints exists for `follow connection` (§14.4) and for the live
# map to see appear and disappear (§25.1, §44.9).
#
#   client.pl <host> <port> <seconds>
#
# It prints `LOCAL=<addr:port> REMOTE=<addr:port>` once connected, so a case can assert on the
# endpoints the kernel actually chose instead of guessing them.

use strict;
use warnings;
use IO::Socket::INET;

my $host    = shift // '127.0.0.1';
my $port    = shift // 18080;
my $seconds = shift // 30;

my $socket = IO::Socket::INET->new(
    PeerAddr => $host,
    PeerPort => $port,
    Proto    => 'tcp',
) or die "fixture: cannot connect to $host:$port: $!\n";

$0 = 'fixture-web-client';
$| = 1;
printf "LOCAL=%s:%s REMOTE=%s:%s\n",
    $socket->sockhost, $socket->sockport, $socket->peerhost, $socket->peerport;

$SIG{TERM} = sub { exit 0 };
sleep $seconds;
exit 0;
